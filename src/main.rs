// Async HTTP boilerplate based on
// https://github.com/daboross/futures-example-2019/

#![recursion_limit = "128"]

pub use anyhow::Result;
use anyhow::{format_err, Error};
use geocoders::cache::Cache;
use geocoders::libpostal::LibPostal;
use geocoders::normalizer::Normalizer;
use geocoders::smarty::Smarty;
use geocoders::{shared_http_client, Geocoder};
use key_value_stores::KeyValueStore;
use leaky_bucket::RateLimiter;
use metrics::describe_counter;
use opinionated_metrics::Mode;
use std::cmp::max;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use structopt::StructOpt;
use tracing::{debug, info_span, warn};
use tracing_subscriber::{
    fmt::{format::FmtSpan, Subscriber},
    prelude::*,
    EnvFilter,
};
use url::Url;

mod addresses;
mod async_util;
mod errors;
mod geocoders;
mod key_value_stores;
mod pipeline;
mod unpack_vec;

use crate::addresses::AddressColumnSpec;
use crate::geocoders::MatchStrategy;
use crate::pipeline::{geocode_stdio, OnDuplicateColumns, CONCURRENCY, GEOCODE_SIZE};

/// Underlying geocoders we can use. (Helper struct for argument parsing.)
#[derive(Clone, Copy, Debug)]
enum GeocoderName {
    Smarty,
    LibPostal,
}

impl FromStr for GeocoderName {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "smarty" => Ok(GeocoderName::Smarty),
            "libpostal" => Ok(GeocoderName::LibPostal),
            _ => Err(format_err!("unknown geocoder {:?}", s)),
        }
    }
}

/// Key/value pairs used to annotate reported metrics. These are of the form
/// `KEY=VALUE`. (Helper struct for argument parsing.)
#[derive(Debug)]
struct MetricsLabel {
    key: String,
    value: String,
}

impl FromStr for MetricsLabel {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        if let Some((key, value)) = s.split_once('=') {
            Ok(MetricsLabel {
                key: key.to_owned(),
                value: value.to_owned(),
            })
        } else {
            Err(format_err!("expected \"key=value\", found {:?}", s))
        }
    }
}

/// Our command-line arguments.
#[derive(Debug, StructOpt)]
#[structopt(about = "geocode CSV files passed on standard input")]
struct Opt {
    /// `strict` for valid postal addresses only, `range` for unknown addresses
    /// within a street's known range, `invalid` to always generate some
    /// match, and `enhanced` (Smarty-only) if you've paid for it.
    #[structopt(long = "match", default_value = "strict")]
    match_strategy: MatchStrategy,

    /// What should we if geocoding output columns have the same names as input
    /// columns? [error, replace, append]
    #[structopt(long = "duplicate-columns", default_value = "error")]
    on_duplicate_columns: OnDuplicateColumns,

    /// A JSON file describing what columns to geocode.
    #[structopt(long = "spec")]
    spec_path: PathBuf,

    /// The geocoder to use.
    #[structopt(long = "geocoder", default_value = "smarty", possible_values = &["smarty", "libpostal"])]
    geocoder: GeocoderName,

    /// What license to use. Leave blank for standard, `us-rooftop-geocoding-enterprise-cloud` for Rooftop.
    #[structopt(
        long = "smarty-license",
        alias = "license",
        default_value = "us-standard-cloud"
    )]
    smarty_license: String,

    /// Cache geocoding results in the specified location (either redis: or
    /// bigtable:).
    #[structopt(long = "cache", value_name = "CACHE_URL")]
    cache_url: Option<Url>,

    /// Include cache keys in the output. Mostly useful for debugging.
    #[structopt(long = "cache-output-keys")]
    cache_output_keys: bool,

    /// Extra prefix to use for cache keys. Should typically end with ":".
    #[structopt(long = "cache-key-prefix", requires = "cache_url")]
    cache_key_prefix: Option<String>,

    /// Before processing addresses, normalize them using libpostal.
    #[structopt(long = "normalize")]
    normalize: bool,

    /// Limit the speed with which we access external geocoding APIs. Does not
    /// affect the cache or local geocoding.
    #[structopt(long = "max-addresses-per-second")]
    max_addresses_per_second: Option<usize>,

    /// Labels to attach to reported metrics. Recommended: "source=$SOURCE".
    #[structopt(long = "metrics-label", value_name = "KEY=VALUE")]
    metrics_labels: Vec<MetricsLabel>,
}

// Our main entrypoint. We rely on the fact that `anyhow::Error` has a `Debug`
// implementation that will print a nice friendly error if we return from `main`
// with an error.
#[tokio::main]
async fn main() -> Result<()> {
    // Configure tracing.
    let filter = EnvFilter::from_default_env();
    Subscriber::builder()
        .with_writer(std::io::stderr)
        .with_span_events(FmtSpan::NEW | FmtSpan::CLOSE)
        .with_env_filter(filter)
        .finish()
        .init();
    let _span = info_span!("geocode-csv").entered();
    debug!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));

    // Parse our command-line arguments.
    let opt = Opt::from_args();
    let spec = AddressColumnSpec::from_path(&opt.spec_path)?;

    // Set up metrics recording.
    let mut metrics_builder = opinionated_metrics::Builder::new(Mode::Cli);
    for label in &opt.metrics_labels {
        metrics_builder = metrics_builder.add_global_label(&label.key, &label.value);
    }
    let metrics_handle = metrics_builder.install()?;

    // Describe our global metrics. Other metrics are described in the modules
    // that use them.
    describe_counter!(
        "geocodecsv.selected_errors.count",
        "Particularly interesting errors, by component and cause"
    );

    // Set up any rate limiting.
    //
    // TODO: If this is low enough, consider reducing our internal parallelism?
    let rate_limiter = opt.max_addresses_per_second.map(|limit| {
        // Always allow geocoding at least one full `GEOCODODE_SIZE`
        // chunk (eventually). We want to make sure that we can
        // accumulate enough tokens to geocode a chunk or two, to
        // prevent a situation where we have a chunk waiting that
        // exceeds our bucket size, blocking it from ever being
        // geocoded.
        let max = max(limit, GEOCODE_SIZE);
        Arc::new(
            RateLimiter::builder()
                .initial(max)
                .max(max)
                .refill(limit)
                .interval(Duration::from_secs(1))
                // Since this is all the same geocoding job,
                .fair(false)
                .build(),
        )
    });

    // Choose our main geocoding client.
    let mut geocoder: Box<dyn Geocoder> = match opt.geocoder {
        GeocoderName::Smarty => Box::new(Smarty::new(
            opt.match_strategy,
            opt.smarty_license.clone(),
            rate_limiter.clone(),
            shared_http_client(CONCURRENCY),
        )?),
        GeocoderName::LibPostal => Box::new(LibPostal::new()),
    };

    // If we were asked, place a cache in front.
    if let Some(cache_url) = &opt.cache_url {
        let cache_key_prefix = opt
            .cache_key_prefix
            .as_deref()
            .unwrap_or_default()
            .to_owned();
        let key_value_store =
            <dyn KeyValueStore>::new_from_url(cache_url.to_owned(), cache_key_prefix)
                .await?;
        geocoder = Box::new(
            Cache::new(key_value_store, geocoder, opt.cache_output_keys).await?,
        );
    }

    // If we were asked, normalize addresses a bit first.
    if opt.normalize {
        geocoder = Box::new(Normalizer::new(geocoder));
    }

    // Call our geocoder.
    let result =
        geocode_stdio(spec, Arc::from(geocoder), opt.on_duplicate_columns).await;

    // Report our metrics.
    if let Err(err) = metrics_handle.report().await {
        warn!("could not report metrics: {:?}", err);
    }

    result
}
