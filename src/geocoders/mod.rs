//! Geocoding backends.

use std::{fmt, iter::repeat, str::FromStr, sync::Arc};

use anyhow::format_err;
use async_trait::async_trait;
use csv::StringRecord;
use hyper::{client::HttpConnector, Client};
use hyper_rustls::{HttpsConnector, HttpsConnectorBuilder};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    addresses::{prefix_column_name, Address},
    Error, Result,
};

pub mod cache;
pub mod invalid_record_skipper;
pub mod libpostal;
pub mod normalizer;
pub mod smarty;

/// A `hyper` client shared between multiple workers.
pub type SharedHttpClient = Arc<Client<HttpsConnector<HttpConnector>>>;

pub fn shared_http_client(concurrency: usize) -> SharedHttpClient {
    // Create a shared `hyper::Client` with a connection pool, so that we can
    // use keep-alive.
    Arc::new(
        Client::builder().pool_max_idle_per_host(concurrency).build(
            HttpsConnectorBuilder::new()
                .with_native_roots()
                .https_only()
                .enable_http2()
                .build(),
        ),
    )
}

/// What match candidates should we output when geocoding?
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchStrategy {
    /// Only match valid USPS addresses.
    Strict,
    /// Match addresses that are within the known range on a street,
    /// but which are not valid USPS addresses.
    Range,
    /// Return a candidate for every address.
    Invalid,
    /// Use "enhanced" matching (which you pay extra for).
    ///
    /// This is an artifact of Smarty, and other compression backends may
    /// treat it as an error.
    Enhanced,
}

// `#derive(Default)` would actually do the right thing, but we want to make the
// default explicit for the reader.
#[allow(clippy::derivable_impls)]
impl Default for MatchStrategy {
    fn default() -> Self {
        MatchStrategy::Strict
    }
}

impl fmt::Display for MatchStrategy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            MatchStrategy::Strict => "strict",
            MatchStrategy::Range => "range",
            MatchStrategy::Invalid => "invalid",
            MatchStrategy::Enhanced => "enhanced",
        };
        s.fmt(f)
    }
}

impl FromStr for MatchStrategy {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        // Do this manually instead of including another library to generate it,
        // or quoting values and parsing them with `serde_json`.
        match s {
            "strict" => Ok(MatchStrategy::Strict),
            "range" => Ok(MatchStrategy::Range),
            "invalid" => Ok(MatchStrategy::Invalid),
            "enhanced" => Ok(MatchStrategy::Enhanced),
            _ => Err(format_err!("unknown match strategy {:?}", s)),
        }
    }
}

/// A geocoded address. This is just a list of values, in the same order as
/// [`Geocoder::column_names`].
#[derive(Clone, Debug, Deserialize)]
pub struct Geocoded {
    /// Column values in the same order as [`Geocoder::column_names`].
    pub column_values: Vec<String>,
}

/// Abstract geocoding interface.
#[async_trait]
pub trait Geocoder: Send + Sync + 'static {
    /// A short name for this geocoder. Typically something like `sm` for
    /// `Smarty` or `lp` for `libpostal`.
    ///
    /// This is used to build cache keys.
    fn tag(&self) -> &str;

    /// Saved configuration as bytes. This may be used as part of a hash table /
    /// cache key. This should always be identical for any two geocoders
    /// configured the same way.
    fn configuration_key(&self) -> &str;

    /// The column names output by this geocoder.
    fn column_names(&self) -> &[String];

    /// The full cache prefix to use for this geocoder.
    ///
    /// This is moderately expensive to compute, so please save it instead of
    /// calling this function repeatedly.
    fn cache_prefix(&self) -> String {
        let mut hasher = Sha256::new();
        for column_name in self.column_names() {
            hasher.update(column_name.as_bytes());
            hasher.update([0]);
        }
        hasher.update(self.configuration_key());
        let hash = hasher.finalize();
        format!("{}:{:02x}{:02x}", self.tag(), hash[0], hash[1],)
    }

    /// Geocode a slice of addresses.
    ///
    /// We return `None` for addresses that we couldn't geocode.
    async fn geocode_addresses(
        &self,
        addresses: &[Address],
    ) -> Result<Vec<Option<Geocoded>>>;

    /// Update a CSV record with our headers, prefixed with `prefix`.
    fn add_header_columns(&self, prefix: &str, out_headers: &mut StringRecord) {
        out_headers.extend(
            self.column_names()
                .iter()
                .map(|column_name| prefix_column_name(prefix, column_name)),
        );
    }

    /// Copy the values from `geocoded` into `out_row`.
    fn add_value_columns_to_row(
        &self,
        geocoded: &Geocoded,
        out_row: &mut StringRecord,
    ) {
        out_row.extend(geocoded.column_values.iter());
    }

    /// Copy empty values into `geocoded`, one for each column that this
    /// geocoder would produce.
    fn add_empty_columns_to_row(&self, out_row: &mut StringRecord) {
        out_row.extend(repeat("").take(self.column_names().len()));
    }
}
