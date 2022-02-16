//! NewRelic client.

use std::collections::HashMap;

use serde_derive::Serialize;
use serde_json::{json, Value};
use tracing::{instrument, trace};

use crate::errors::ReportError;

/// An extremely light-weight client for NewRelic.
pub(crate) struct NewRelicClient {
    api_key: String,
    http_client: reqwest::Client,
}

impl NewRelicClient {
    pub(crate) fn new(api_key: String) -> NewRelicClient {
        NewRelicClient {
            api_key,
            http_client: reqwest::Client::new(),
        }
    }

    /// Report the specified metrics to NewRelic using their [API][].
    ///
    /// [api]: https://docs.newrelic.com/docs/data-apis/ingest-apis/metric-api/report-metrics-metric-api/
    #[instrument(
        name = "NewRelicClient::report_metrics",
        level = "debug",
        skip_all,
        fields(
            metrics.len = report.iter().map(|r| r.metrics.len()).sum::<usize>(),
        ),
    )]
    pub(crate) async fn report_metrics(
        &self,
        report: &[MetricsReport],
    ) -> Result<(), ReportError> {
        trace!("Reporting metrics: {}", json!(report));
        let response = self
            .http_client
            .post("https://metric-api.newrelic.com/metric/v1")
            .header("Api-Key", &self.api_key)
            .header("Content-Type", "application/json")
            .json(report)
            .send()
            .await
            .map_err(ReportError::from_error)?;
        if response.status().is_success() {
            Ok(())
        } else {
            Err(ReportError::from_response(response).await)
        }
    }
}

/// A [metrics report][api] to NewRelic.
///
/// [api]: https://docs.newrelic.com/docs/data-apis/ingest-apis/metric-api/report-metrics-metric-api/
#[derive(Debug, Serialize)]
pub(crate) struct MetricsReport {
    pub(crate) common: MetricsCommon,
    pub(crate) metrics: Vec<Metric>,
}

/// Common information about all metrics.
#[derive(Debug, Serialize)]
pub(crate) struct MetricsCommon {
    #[serde(rename = "interval.ms")]
    pub(crate) interval_ms: u64,
    pub(crate) timestamp: u64,
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub(crate) attributes: HashMap<String, Value>,
}

/// Information about a specific metric.
#[derive(Debug, Serialize)]
pub(crate) struct Metric {
    pub(crate) name: String,
    pub(crate) value: Value,
    #[serde(rename = "type")]
    pub(crate) type_: MetricType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) attributes: Option<HashMap<String, Value>>,
}

/// NewRelic [metric types][].
///
/// [metric types]:
///     https://docs.newrelic.com/docs/data-apis/understand-data/metric-data/metric-data-type/
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub(crate) enum MetricType {
    Count,
    Distribution,
    Gauge,
    Summmary,
    UniqueCount,
}
