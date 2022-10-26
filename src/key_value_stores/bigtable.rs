//! Support for using BigTable as a key/value store.

use std::{
    borrow::Cow,
    collections::HashMap,
    time::{Duration, Instant},
};

use anyhow::{format_err, Context};
use async_trait::async_trait;
use bigtable_rs::{
    bigtable::{self, BigTable as BigTableClient, BigTableConnection},
    google::bigtable::v2::{
        mutate_rows_request::Entry,
        mutation::{self, SetCell},
        row_filter::{Chain, Filter},
        MutateRowsRequest, Mutation, ReadRowsRequest, RowFilter, RowSet,
    },
};
use metrics::{counter, describe_histogram, histogram, Unit};
use tracing::{instrument, trace};
use url::Url;

use crate::{pipeline::CONCURRENCY, Result};

use super::{KeyValueStore, KeyValueStoreNew, PipelinedGet, PipelinedSet};

const GEOCODE_CSV_FAMILY_NAME: &str = "geocode_csv";
const GEOCODE_CSV_COLUMN_NAME: &[u8] = b"v";

/// BigTable configuration information.
struct BigTableConfig {
    project_id: String,
    instance_id: String,
    table_name: String,
}

impl BigTableConfig {
    /// Parse our own fake "bigtable:" URL schema into configuration information.
    fn from_url(url: &Url) -> Result<BigTableConfig> {
        if url.scheme() == "bigtable" {
            let domain = url.host_str();
            if let Some(domain) = domain {
                if let Some(segments) = url.path_segments() {
                    let segments = segments.collect::<Vec<_>>();
                    if segments.len() == 2 {
                        // We have a valid "URL"! (Not really, because the
                        // "bigtable" scheme doesn't exist.
                        return Ok(BigTableConfig {
                            project_id: domain.to_owned(),
                            instance_id: segments[0].to_owned(),
                            table_name: segments[1].to_owned(),
                        });
                    }
                }
            }
        }

        Err(format_err!(
            "expected bigtable:// URL, found {:?}",
            url.scheme()
        ))
    }
}

#[test]
fn bigtable_config_from_url() {
    let examples = &[(
        "bigtable://project-123/instance-456/table-789",
        "project-123",
        "instance-456",
        "table-789",
    )];

    for (url, project_id, instance_id, table_id) in examples.iter() {
        let url = Url::parse(url).unwrap();
        let config = BigTableConfig::from_url(&url).unwrap();
        assert_eq!(config.project_id, *project_id);
        assert_eq!(config.instance_id, *instance_id);
        assert_eq!(config.table_name, *table_id);
    }
}

pub struct BigTable {
    /// Our BigTable gRPC connection pool.
    connection: BigTableConnection,

    /// The table in which we're storing our keys.
    table_name: String,

    /// The prefix to use for our keys.
    key_prefix: String,
}

impl BigTable {
    /// Get a BigTable client.
    fn client(&self) -> BigTableClient {
        self.connection.client()
    }
}

impl KeyValueStore for BigTable {
    fn new_pipelined_get<'store>(
        &'store self,
    ) -> Box<dyn PipelinedGet<'store> + 'store> {
        Box::new(BigTablePipelinedGet {
            bigtable: self,
            row_keys: vec![],
        })
    }

    fn new_pipelined_set<'store>(
        &'store self,
    ) -> Box<dyn PipelinedSet<'store> + 'store> {
        Box::new(BigTablePipelinedSet {
            bigtable: self,
            entries: vec![],
        })
    }

    fn key_prefix(&self) -> &str {
        &self.key_prefix
    }
}

#[async_trait]
impl KeyValueStoreNew for BigTable {
    /// Create a new key/value store client.
    #[instrument(level = "debug", skip_all)]
    async fn new(url: Url, key_prefix: String) -> Result<Self> {
        describe_histogram!(
            "geocodecsv.bigtable.get_request.duration_seconds",
            Unit::Seconds,
            "Time required for BigTable read_rows requests"
        );
        describe_histogram!(
            "geocodecsv.bigtable.set_request.duration_seconds",
            Unit::Seconds,
            "Time required for BigTable MutateRows requests"
        );

        let config = BigTableConfig::from_url(&url)?;
        let connection = BigTableConnection::new(
            &config.project_id,
            &config.instance_id,
            /* read_only */ false,
            CONCURRENCY,
            Some(Duration::from_secs(60)),
        )
        .await
        .context("could not connect to BigTable")?;

        Ok(BigTable {
            connection,
            table_name: config.table_name,
            key_prefix,
        })
    }
}

/// A batched GET request.
struct BigTablePipelinedGet<'store> {
    bigtable: &'store BigTable,
    row_keys: Vec<Vec<u8>>,
}

/// A series of "get" requests that we'll send in a single batch.
#[async_trait]
impl<'store> PipelinedGet<'store> for BigTablePipelinedGet<'store> {
    fn add_get(&mut self, key: String) {
        trace!("bigtable: reading {}", key);
        self.row_keys.push(key.into_bytes());
    }

    #[instrument(name = "PipelinedGet::execute", level = "trace", skip_all, fields(row_keys.len = self.row_keys.len()))]
    async fn execute(&self) -> Result<Vec<Option<Vec<u8>>>> {
        let start = Instant::now();

        // Build and run our query.
        let mut client = self.bigtable.client();
        let request = ReadRowsRequest {
            table_name: client.get_full_table_name(&self.bigtable.table_name),
            rows: Some(RowSet {
                row_keys: self.row_keys.to_owned(),
                row_ranges: vec![],
            }),
            filter: Some(RowFilter {
                filter: Some(Filter::Chain(Chain {
                    filters: vec![
                        RowFilter {
                            filter: Some(Filter::FamilyNameRegexFilter(
                                GEOCODE_CSV_FAMILY_NAME.to_owned(),
                            )),
                        },
                        RowFilter {
                            filter: Some(Filter::ColumnQualifierRegexFilter(
                                GEOCODE_CSV_COLUMN_NAME.to_vec(),
                            )),
                        },
                        RowFilter {
                            filter: Some(Filter::CellsPerColumnLimitFilter(1)),
                        },
                    ],
                })),
            }),
            ..ReadRowsRequest::default()
        };
        trace!("bigtable request: {:?}", request);
        let response = match client.read_rows(request).await {
            Ok(response) => response,
            Err(err) => {
                let cause = bigtable_error_cause_for_metrics(&err);
                counter!("geocodecsv.selected_errors.count", 1, "component" => "bigtable", "cause" => cause);
                return Err(err).context("error checking BigTable for cached values");
            }
        };

        histogram!(
            "geocodecsv.bigtable.get_request.duration_seconds",
            (Instant::now() - start).as_secs_f64(),
        );

        // Build a vec to store our result, and a `HashMap` mapping keys to vec
        // indices. We have to be careful because keys might appear more than
        // once. Ideally our callers wouldn't do that, but if they do, we need
        // to handle it.
        let mut result: Vec<Option<Vec<u8>>> = vec![None; self.row_keys.len()];
        let mut row_key_indices = HashMap::<Vec<u8>, Vec<usize>>::new();
        for (idx, row_key) in self.row_keys.iter().enumerate() {
            row_key_indices
                .entry(row_key.clone())
                .or_default()
                .push(idx);
        }

        // Extract the found keys and store them in our result.
        for (key, data) in response {
            for row_cell in data {
                trace!(
                    "bigtable row found: [{:?}:{:?}]={:?} @ {}",
                    row_cell.family_name,
                    String::from_utf8_lossy(&row_cell.qualifier),
                    row_cell.value,
                    row_cell.timestamp_micros,
                );

                // Check to make sure we got the right data.
                if row_cell.family_name != GEOCODE_CSV_FAMILY_NAME {
                    return Err(format_err!(
                        "expected column family name {:?}, found {:?}",
                        GEOCODE_CSV_FAMILY_NAME,
                        row_cell.family_name,
                    ));
                }
                if row_cell.qualifier != GEOCODE_CSV_COLUMN_NAME {
                    return Err(format_err!(
                        "expected qualifier {:?}, found {:?}",
                        GEOCODE_CSV_COLUMN_NAME,
                        row_cell.qualifier,
                    ));
                }

                // Write this match to our result array.
                let indices = row_key_indices
                    .get(&key)
                    .expect("we should always have a known key");
                for idx in indices {
                    result[*idx] = Some(row_cell.value.clone());
                }
            }
        }
        Ok(result)
    }
}

/// A batched SET request.
struct BigTablePipelinedSet<'store> {
    bigtable: &'store BigTable,
    entries: Vec<Entry>,
}

/// A series of "set" requests that we'll send in a single batch.
#[async_trait]
impl<'store> PipelinedSet<'store> for BigTablePipelinedSet<'store> {
    fn add_set(&mut self, key: String, value: Vec<u8>) {
        trace!("bigtable: writing {} ({} bytes)", key, value.len());
        self.entries.push(Entry {
            row_key: key.into_bytes(),
            mutations: vec![Mutation {
                mutation: Some(mutation::Mutation::SetCell(SetCell {
                    family_name: GEOCODE_CSV_FAMILY_NAME.to_owned(),
                    column_qualifier: GEOCODE_CSV_COLUMN_NAME.to_owned(),
                    timestamp_micros: -1,
                    value,
                })),
            }],
        })
    }

    #[instrument(name = "PipelinedSet::execute", level = "trace", skip_all, fields(entries.len = self.entries.len()))]
    async fn execute(&self) -> Result<()> {
        let start = Instant::now();

        // Build and run our query.
        let mut client = self.bigtable.client();
        let request = MutateRowsRequest {
            table_name: client.get_full_table_name(&self.bigtable.table_name),
            app_profile_id: "".to_owned(), // Dave says this is what we want.
            entries: self.entries.clone(),
        };
        if let Err(err) = client.mutate_rows(request).await {
            let cause = bigtable_error_cause_for_metrics(&err);
            counter!("geocodecsv.selected_errors.count", 1, "component" => "bigtable", "cause" => cause);
            return Err(err).context("error writing cached values to BigTable");
        }

        histogram!(
            "geocodecsv.bigtable.set_request.duration_seconds",
            (Instant::now() - start).as_secs_f64(),
        );
        Ok(())
    }
}

/// Convert a BigTable error to a "low-arity" string describing what went wrong.
///
/// For reporting with metrics. This does not replace detailed logs, but it
/// should give some quick ideas about what to look for.
fn bigtable_error_cause_for_metrics(err: &bigtable::Error) -> Cow<'static, str> {
    match err {
        bigtable::Error::AccessTokenError(_) => Cow::Borrowed("access token"),
        bigtable::Error::CertificateError(_) => Cow::Borrowed("certificate"),
        // `std:io::ErrorKind` is low-arity, and Rust can at least turn it into
        // a `Debug` string.
        bigtable::Error::IoError(err) => Cow::Owned(format!("{:?}", err.kind())),
        bigtable::Error::TransportError(_) => Cow::Borrowed("transport"),
        bigtable::Error::RowNotFound => Cow::Borrowed("row not found"),
        bigtable::Error::RowWriteFailed => Cow::Borrowed("row write failed"),
        bigtable::Error::ObjectNotFound(_) => Cow::Borrowed("object not found"),
        bigtable::Error::ObjectCorrupt(_) => Cow::Borrowed("object corrupt"),
        bigtable::Error::RpcError(_) => Cow::Borrowed("rpc"),
        bigtable::Error::TimeoutError(_) => Cow::Borrowed("timeout"),
        bigtable::Error::ChunkError(_) => Cow::Borrowed("chunk"),
        bigtable::Error::GCPAuthError(_) => Cow::Borrowed("gcp auth"),
    }
}
