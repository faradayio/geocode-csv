#![recursion_limit = "128"]

pub use anyhow::Result;
use anyhow::{format_err, Error};
use clap::{Parser, Subcommand, ValueEnum};
use leaky_bucket::RateLimiter;
use metrics::describe_counter;
use opinionated_metrics::Mode;

use std::cmp::max;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
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
#[cfg(debug_assertions)]
mod memory_used;
mod pipeline;
mod server;
mod unpack_vec;

use crate::geocoders::cache::refresh::{RefreshPolicy, RefreshPolicyConfig};
use crate::geocoders::{
    cache::Cache, invalid_record_skipper::InvalidRecordSkipper, libpostal::LibPostal,
    normalizer::Normalizer, shared_http_client, smarty::Smarty, Geocoder,
    MatchStrategy,
};
use crate::key_value_stores::KeyValueStore;
use crate::pipeline::{geocode_stdio, OnDuplicateColumns, CONCURRENCY, GEOCODE_SIZE};
use crate::server::run_server;
use crate::{addresses::AddressColumnSpec, geocoders::paired::Paired};

#[cfg(all(feature = "tikv-jemallocator", not(target_env = "msvc")))]
#[global_allocator]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

/// Underlying geocoders we can use. (Helper struct for argument parsing.)
#[derive(Clone, Copy, Debug, ValueEnum)]
enum GeocoderName {
    #[value(name = "smarty")]
    Smarty,
    #[value(name = "libpostal")]
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
#[derive(Clone, Debug)]
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

/// Convert a whole-day count from a CLI flag into a `Duration`.
fn duration_from_days(days: u64) -> Result<Duration> {
    Ok(Duration::from_secs(
        days.checked_mul(24 * 60 * 60).ok_or_else(|| {
            format_err!("day count {days} is too large to convert to a duration")
        })?,
    ))
}

/// Our command-line arguments.
#[derive(Debug, Parser)]
#[command(author, version, about = "geocode CSV files passed on standard input")]
struct Opt {
    /// `strict` for valid postal addresses only, `range` for unknown addresses
    /// within a street's known range, `invalid` to always generate some
    /// match, and `enhanced` (Smarty-only) if you've paid for it.
    #[arg(long = "match", default_value = "strict")]
    match_strategy: MatchStrategy,

    /// What should we if geocoding output columns have the same names as input
    /// columns? [error, replace, append]
    #[arg(long = "duplicate-columns", default_value = "error")]
    on_duplicate_columns: OnDuplicateColumns,

    /// A JSON file describing what columns to geocode.
    #[arg(long = "spec")]
    spec_path: PathBuf,

    /// The geocoder to use.
    #[arg(long = "geocoder", default_value = "smarty")]
    geocoder: GeocoderName,

    /// What license to use. Leave blank for standard, `us-rooftop-geocoding-enterprise-cloud` for Rooftop.
    #[arg(
        long = "smarty-license",
        alias = "license",
        default_value = "us-standard-cloud"
    )]
    smarty_license: String,

    /// Cache geocoding results in the specified location (either redis: or
    /// bigtable:).
    #[arg(long = "cache", value_name = "CACHE_URL")]
    cache_url: Option<Url>,

    /// Whether or not cache misses should be geocoded.
    #[arg(long = "cache-hits-only")]
    cache_hits_only: bool,

    /// Include cache keys in the output. Mostly useful for debugging.
    #[arg(long = "cache-output-keys")]
    cache_output_keys: bool,

    /// Extra prefix to use for cache keys. Should typically end with ":".
    #[arg(long = "cache-key-prefix", requires = "cache_url")]
    cache_key_prefix: Option<String>,

    /// Re-geocode a cached failure after this many days (then 2x, 4x, …).
    /// Requires `--refresh-failures-max-attempts`, `--refresh-rate`, and
    /// `--cache=bigtable://`.
    #[arg(
        long = "refresh-failures-after-days",
        value_name = "N",
        requires = "refresh_failures_max_attempts"
    )]
    refresh_failures_after_days: Option<u64>,

    /// Stop re-checking a cached failure after this many refreshes. Requires
    /// `--refresh-failures-after-days`.
    #[arg(
        long = "refresh-failures-max-attempts",
        value_name = "N",
        requires = "refresh_failures_after_days"
    )]
    refresh_failures_max_attempts: Option<u32>,

    /// Re-geocode a cached success after this many days. Requires
    /// `--refresh-rate` and `--cache=bigtable://`.
    #[arg(long = "refresh-successes-after-days", value_name = "N")]
    refresh_successes_after_days: Option<u64>,

    /// Fraction of eligible cache keys to refresh on a given day, in (0, 1].
    /// Required whenever any `--refresh-*` period is set.
    #[arg(long = "refresh-rate", value_name = "F")]
    refresh_rate: Option<f64>,

    /// Before processing addresses, normalize them using libpostal.
    #[arg(long = "normalize")]
    normalize: bool,

    /// Include libpostal columns in addition to another geocoder's output.
    #[arg(long = "include-libpostal")]
    include_libpostal: bool,

    /// Limit the speed with which we access external geocoding APIs. Does not
    /// affect the cache or local geocoding.
    #[arg(long = "max-addresses-per-second")]
    max_addresses_per_second: Option<usize>,

    /// How many times should we retry a failed geocoding block? Each retry
    /// takes twice as long as the last. The current default value will result
    /// in giving up after about 30 seconds.
    #[arg(long = "max-retries", default_value = "4")]
    max_retries: u8,

    /// Labels to attach to reported metrics. Recommended: "source=$SOURCE".
    #[arg(long = "metrics-label", value_name = "KEY=VALUE")]
    metrics_labels: Vec<MetricsLabel>,

    /// Command to run.
    #[command(subcommand)]
    cmd: Option<Command>,
}

/// Subcommands for geocode-csv.
#[derive(Debug, Subcommand)]
enum Command {
    /// Start in server mode.
    Server {
        /// Address that the server should listen on.
        #[arg(long = "listen-address", default_value = "127.0.0.1:8787")]
        listen_address: String,
    },
}

// Our main entrypoint. We rely on the fact that `anyhow::Error` has a `Debug`
// implementation that will print a nice friendly error if we return from `main`
// with an error.
#[tokio::main]
async fn main() -> Result<()> {
    // Initialize rustls crypto provider before any TLS operations.
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .map_err(|_| anyhow::anyhow!("Failed to install rustls crypto provider"))?;

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
    let opt = Opt::parse();
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
                // The docs recommend twice our refill rate or our
                // initial value, whichever is larger.
                .max(2 * max)
                .refill(limit)
                .interval(Duration::from_secs(1))
                // Since this is all the same geocoding job, don't worry about
                // fair scheduling between different worker tasks.
                .fair(false)
                .build(),
        )
    });

    // Build our cache refresh policy, if requested. Refresh is only supported
    // by the BigTable cache, which records a write time per entry.
    let wants_refresh = opt.refresh_failures_after_days.is_some()
        || opt.refresh_successes_after_days.is_some();
    let refresh_policy = if wants_refresh {
        let Some(rate) = opt.refresh_rate else {
            return Err(format_err!(
                "--refresh-rate is required whenever a --refresh-* period is set"
            ));
        };
        if !(rate > 0.0 && rate <= 1.0) {
            return Err(format_err!("--refresh-rate must be in (0, 1], got {rate}"));
        }
        if let Some(days) = opt.refresh_failures_after_days {
            if days == 0 {
                return Err(format_err!(
                    "--refresh-failures-after-days must be greater than 0"
                ));
            }
        }
        if let Some(max_attempts) = opt.refresh_failures_max_attempts {
            if max_attempts == 0 {
                return Err(format_err!(
                    "--refresh-failures-max-attempts must be greater than 0"
                ));
            }
        }
        if let Some(days) = opt.refresh_successes_after_days {
            if days == 0 {
                return Err(format_err!(
                    "--refresh-successes-after-days must be greater than 0"
                ));
            }
        }
        match &opt.cache_url {
            Some(cache_url) if cache_url.scheme() == "bigtable" => {}
            Some(_) => {
                return Err(format_err!(
                    "--refresh-* flags are only supported with --cache=bigtable://"
                ))
            }
            None => {
                return Err(format_err!(
                    "--refresh-* flags require --cache=bigtable://"
                ))
            }
        }
        Some(RefreshPolicy::new(RefreshPolicyConfig {
            failure_period: opt
                .refresh_failures_after_days
                .map(duration_from_days)
                .transpose()?,
            failure_max_attempts: opt.refresh_failures_max_attempts.unwrap_or(0),
            success_period: opt
                .refresh_successes_after_days
                .map(duration_from_days)
                .transpose()?,
            rate,
        }))
    } else if opt.refresh_rate.is_some() {
        return Err(format_err!(
            "--refresh-rate requires --refresh-failures-after-days or --refresh-successes-after-days"
        ));
    } else {
        None
    };

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
            Cache::new(
                key_value_store,
                geocoder,
                opt.cache_output_keys,
                opt.cache_hits_only,
                refresh_policy,
            )
            .await?,
        );
    }

    // Always skip invalid records. This needs to happen after we do
    // normalization, because normalization might move data between fields.
    geocoder = Box::new(InvalidRecordSkipper::new(geocoder));

    // If we were asked, normalize addresses a bit first.
    if opt.normalize {
        geocoder = Box::new(Normalizer::new(geocoder));
    }

    // Include libpostal columns in the output if requested.
    if opt.include_libpostal {
        geocoder = Box::new(Paired::new(
            geocoder,
            "libpostal",
            Box::new(LibPostal::new()),
        ));
    }

    // Decide which command to run.
    let result = match opt.cmd {
        // Run in server mode.
        Some(Command::Server { listen_address }) => {
            // If we're running in server mode, then prime libpostal to load its
            // model and data into memory. This can take 5-10 seconds,
            // and we'd prefer that it happens as part of application startup,
            // rather than at the time of the first request.
            LibPostal::prime().await;
            run_server(&listen_address, geocoder).await
        }
        // Run in CLI pipeline mode.
        None => {
            geocode_stdio(
                spec,
                Arc::from(geocoder),
                opt.on_duplicate_columns,
                opt.max_retries,
            )
            .await
        }
    };

    // Report our metrics.
    if let Err(err) = metrics_handle.report().await {
        warn!("could not report metrics: {:?}", err);
    }

    result
}
