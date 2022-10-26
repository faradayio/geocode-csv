//! A simple interface for reporting `metrics` data to NewRelic.
//!
//! We have a number of limitations:
//!
//! - We do not provide any automatic way to submitted data on a scheduled
//!   interval.
//! - We may or may not support `histogram!` as a NewRelic `summary` metric. We
//!   _do_ send the data. But it doesn't show up and it doesn't report
//!   errors in `NrIntegrationError`.
//! - We always report all metrics that we've ever seen. We could stop reporting
//!   stale metrics if we figured out [`metrics_util::Recency`].
//!
//! ## Example code
//!
//! ```no_run
//! # async fn example() -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
//! use metrics_exporter_newrelic::NewRelicBuilder;
//!
//! // Create and install our metrics reporter.
//! let handle = NewRelicBuilder::new("my-api-key")
//!     .add_global_label("host.name", "server1.example.com")
//!     .build()?
//!     .install()?;
//!
//! // Report metrics to NewRelic whenever you want.
//! handle.report().await?;
//! # Ok(()) }
//! ```

use std::{
    collections::HashMap,
    sync::{atomic::Ordering, Arc},
    time::{SystemTime, UNIX_EPOCH},
};

use errors::ReportError;
use metrics::{Counter, Gauge, Histogram, Key, KeyName, Recorder, SharedString, Unit};
use metrics_util::registry::{AtomicStorage, Registry};
use serde_json::{json, Value};
use tokio::sync::RwLock;
use tracing::instrument;

mod client;
mod errors;

use crate::client::{MetricsCommon, MetricsReport, NewRelicClient};
pub use crate::errors::BuildError;

/// Given a function `f` (which is typically either `min` or `max`):
///
/// 1. If both values are non-`None`, return `Some(f(v1, v2))` .
/// 2. If only one value is non-`None`, return it.
/// 3. Otherwise, return `None`.
///
/// We use this for reducing metrics cleanly.
fn reduce_discarding_nulls<F, T>(f: F, v1: Option<T>, v2: Option<T>) -> Option<T>
where
    F: FnOnce(T, T) -> T,
{
    match (v1, v2) {
        (None, None) => None,
        (Some(v), None) => Some(v),
        (None, Some(v)) => Some(v),
        (Some(v1), Some(v2)) => Some(f(v1, v2)),
    }
}

/// Builder for creating and installing a `NewRelicRecoder` and exporter.
pub struct NewRelicBuilder {
    /// Our NewRelic API key.
    api_key: String,

    /// Default labels to attach to every metric we report.
    global_labels: HashMap<String, Value>,
}

impl NewRelicBuilder {
    /// Create a `NewRelicBuilder`, including our API key.
    pub fn new<S: Into<String>>(api_key: S) -> Self {
        NewRelicBuilder {
            api_key: api_key.into(),
            global_labels: HashMap::new(),
        }
    }

    /// Add a label which will be attached to all metrics by default.
    pub fn add_global_label<K, V>(mut self, key: K, value: V) -> Self
    where
        K: Into<String>,
        V: Into<Value>,
    {
        self.global_labels.insert(key.into(), value.into());
        self
    }

    /// Construct a `NewRelicRecorder` with the specified parameters.
    pub fn build(self) -> Result<NewRelicRecorder, BuildError> {
        Ok(NewRelicRecorder {
            inner: Arc::new(Inner {
                registry: Registry::new(AtomicStorage),
                api_key: self.api_key,
                global_labels: self.global_labels,
                metadata: RwLock::new(MetricMetadata::new()),
            }),
        })
    }
}

/// Metadata we need to store about metrics in order to correctly report to
/// NewRelic.
struct MetricMetadata {
    /// The last time we reported metrics to NewRelic.
    previous_timestamp: SystemTime,

    /// NewRelic wants us to reset our counts to zero every time we report.
    /// This is the value that we most recently reported.
    previously_reported_counts: HashMap<Key, u64>,
}

impl MetricMetadata {
    fn new() -> MetricMetadata {
        MetricMetadata {
            previous_timestamp: SystemTime::now(),
            previously_reported_counts: HashMap::new(),
        }
    }
}

/// The actual implementation of `NewRelicRecorder`.
///
/// We keep this behind `Inner` so that we can access it even after it has been
/// installed.
struct Inner {
    /// The registry which manages the low-level details of our metrics.
    registry: Registry<Key, AtomicStorage>,

    /// Our NewRelic API key.
    api_key: String,

    /// Global labels to add to each metric.
    global_labels: HashMap<String, Value>,

    /// Metadata about our metrics.
    metadata: RwLock<MetricMetadata>,
}

impl Inner {
    /// Report metrics to NewRelic.
    #[instrument(name = "NewRelicHandle::report", skip_all)]
    async fn report(&self) -> Result<(), ReportError> {
        // Lock our metadata during each report, so we accurately remember what
        // we've told NewRelic. Note that we _could_ exit silently at any
        // `await` if our future is cancelled. So we must leave locked data in a
        // "clean" state over any `await`.
        let mut metadata = self.metadata.write().await;
        let now = SystemTime::now();

        let interval_ms = now
            .duration_since(metadata.previous_timestamp)
            // If time has moved backwards, use an interval of 0.
            .unwrap_or_default()
            .as_millis() as u64;
        let timestamp = metadata
            .previous_timestamp
            .duration_since(UNIX_EPOCH)
            // Round everything before 1970-01-01 to 0.
            .unwrap_or_default()
            .as_secs();

        // Extract information about our metrics from our registry. We allow
        // `mutable_key_type` here for reasons explained in `metrics::Key`.
        let mut metrics = vec![];
        #[allow(clippy::mutable_key_type)]
        let mut reported_counts = HashMap::new();
        for (key, counter) in self.registry.get_counter_handles() {
            let previous = metadata
                .previously_reported_counts
                .get(&key)
                .cloned()
                .unwrap_or(0);
            let current = counter.load(Ordering::Acquire);
            let reported_count = current - previous;

            metrics.push(client::Metric {
                name: key.name().to_owned(),
                value: json!(reported_count),
                type_: client::MetricType::Count,
                attributes: attributes_from_key(&key),
            });

            reported_counts.insert(key, reported_count);
        }
        for (key, gauge) in self.registry.get_gauge_handles() {
            let current = gauge.load(Ordering::Acquire);
            metrics.push(client::Metric {
                name: key.name().to_owned(),
                value: json!(current),
                type_: client::MetricType::Gauge,
                attributes: attributes_from_key(&key),
            });
        }
        for (key, bucket) in self.registry.get_histogram_handles() {
            // Iterate over all our datapoints, clearing them, and computing the
            // only summary statistics NewRelic is documented to accept.
            //
            // WARNING: Unlike with counters above, we may lose data if metric
            // submission fails. We do not attempt to preserve data which failed
            // to submit.
            let mut count: usize = 0;
            let mut sum: f64 = 0.0;
            let mut min: Option<f64> = None;
            let mut max: Option<f64> = None;
            bucket.clear_with(|values| {
                count += values.len();
                for value in values {
                    sum += value;
                    min = reduce_discarding_nulls(f64::min, min, Some(*value));
                    max = reduce_discarding_nulls(f64::max, max, Some(*value));
                }
            });

            if count > 0 {
                debug_assert!(min.is_some() && max.is_some());
                metrics.push(client::Metric {
                    name: key.name().to_owned(),
                    value: json!({
                        "count": count,
                        "sum": sum,
                        "min": min,
                        "max": max,
                    }),
                    type_: client::MetricType::Summmary,
                    attributes: attributes_from_key(&key),
                });
            }
        }

        // Report our metrics. Note that if this fails, we drop our lock on `metadata`.
        let metrics_report = MetricsReport {
            common: MetricsCommon {
                interval_ms,
                timestamp,
                attributes: self.global_labels.clone(),
            },
            metrics,
        };
        let client = NewRelicClient::new(self.api_key.clone());
        client.report_metrics(&[metrics_report]).await?;

        // Update our metadata. We only do this _after_ reporting, so that we
        // don't lose any metrics if we can't contact NewRelic.
        for (key, count) in reported_counts {
            metadata.previously_reported_counts.insert(key, count);
        }

        Ok(())
    }
}

/// Convert `metrics` key attributes to NewRelic attributes.
fn attributes_from_key(key: &Key) -> Option<HashMap<String, Value>> {
    let labels = key.labels();
    if labels.len() == 0 {
        None
    } else {
        Some(HashMap::from_iter(
            labels.map(|label| (label.key().to_owned(), json!(label.value()))),
        ))
    }
}

/// A metrics recorder that reports to NewRelic.
pub struct NewRelicRecorder {
    /// Our actual data, in a shared structure.
    inner: Arc<Inner>,
}

impl NewRelicRecorder {
    /// Get a handle which can be used to access certain reporter features after
    /// it's installed.
    pub fn handle(&self) -> NewRelicHandle {
        NewRelicHandle {
            inner: self.inner.clone(),
        }
    }

    /// Install this recorder as the global default recorder.
    pub fn install(self) -> Result<NewRelicHandle, BuildError> {
        let handle = self.handle();
        metrics::set_boxed_recorder(Box::new(self))?;
        Ok(handle)
    }
}

impl Recorder for NewRelicRecorder {
    fn describe_counter(
        &self,
        _key: KeyName,
        _unit: Option<Unit>,
        _description: SharedString,
    ) {
        // NewRelic does not use this information.
    }

    fn describe_gauge(
        &self,
        _key: KeyName,
        _unit: Option<Unit>,
        _description: SharedString,
    ) {
        // NewRelic does not use this information.
    }

    fn describe_histogram(
        &self,
        _key: KeyName,
        _unit: Option<Unit>,
        _description: SharedString,
    ) {
        // NewRelic does not use this information.
    }

    fn register_counter(&self, key: &Key) -> Counter {
        self.inner
            .registry
            .get_or_create_counter(key, |c| c.clone().into())
    }

    fn register_gauge(&self, key: &Key) -> Gauge {
        self.inner
            .registry
            .get_or_create_gauge(key, |c| c.clone().into())
    }

    fn register_histogram(&self, key: &Key) -> Histogram {
        self.inner
            .registry
            .get_or_create_histogram(key, |c| c.clone().into())
    }
}

/// A handle to a `NewRelicRecorder`. You can use this to access certain
/// features of the recorder after installing it.
pub struct NewRelicHandle {
    inner: Arc<Inner>,
}

impl NewRelicHandle {
    /// Report the current metrics values to NewRelic.
    pub async fn report(&self) -> Result<(), ReportError> {
        self.inner.report().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_reporter() {
        NewRelicBuilder::new("my-api-key")
            .add_global_label("host.name", "server1.example.com")
            .build()
            .unwrap();
    }
}
