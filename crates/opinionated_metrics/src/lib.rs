//! An "opinioned" interface to the
//! [`metrics`](https://docs.rs/metrics/latest/metrics/) library.
//!
//! Our goal is to provide a set of sensible defaults, so that:
//!
//! 1. Programs using `opinionated_metrics` can set up metrics reporting with
//!    just a few lines of code.
//! 2. Users have one consistent way to configure metrics reporting.
//!
//! ## Features
//!
//! Here's what's implemented, and what might get done depending on what the
//! maintainers need at work.
//!
//! - Backends
//!   - [x] Logging
//!   - [x] NewRelic
//!   - [ ] Prometheus
//!   - (PRs for other popular backends will be considered!)
//! - Modes
//!   - [x] Metrics reporting for CLI tools.
//!   - [ ] Metrics reporting for servers.
//! - [ ] Automatic reporting at scheduled intervals.
//!
//!
//! ## Example (CLI)
//!
//! ```no_run
//! # async fn example() -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
//! // Set up metrics reporting for a CLI tool, using environment variables.
//! let metrics_handle = opinionated_metrics::initialize_cli()?;
//!
//! // Do stuff.
//!
//! // Send a metrics report when the CLI tool has finished running.
//! metrics_handle.report().await?;
//! # Ok(()) }
//! ```
//!
//! ## Configuration
//!
//! Depending on what environment variables are set, we will use different
//! reporting backends:
//!
//! - `NEW_RELIC_API_KEY`: Report metrics to NewRelic.
//! - None of the above: Log metrics in Prometheus format using `tracing::info`.
//!
//! ## Metric naming conventions
//!
//! For best results across different metrics reporting systems, we recommend
//! following the [Prometheus metric naming
//! conventions](https://prometheus.io/docs/practices/naming/), except using
//! periods to separate the different components of the name.
//!
//! Example metric names:
//!
//! - `myapp.requests.total`: Counter with no units.
//! - `myapp.processed.bytes_total`: Counter with units.
//! - `myapp.memory_usage.bytes`: Gauge with units.
//!
//! ## Label naming
//!
//! Labels should be "low-arity". Specifically, that means that labels should
//! have only a small number of possible values, because each possible label
//! value will require most backends to store a new time series.

use std::{collections::HashMap, env};

use metrics_exporter_newrelic::{NewRelicBuilder, NewRelicHandle};
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use thiserror::Error;
use tracing::info;

/// A metrics-related error
///
/// We return errors from multiple underlying libraries, so our error type is
/// opaque.
#[derive(Debug, Error)]
#[error("could not report metrics")]
pub struct Error {
    source: Box<dyn std::error::Error + Send + Sync + 'static>,
}

impl Error {
    fn new<E>(err: E) -> Error
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        Error {
            source: Box::new(err),
        }
    }
}

/// What mode (CLI or server) should we use?
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Mode {
    /// Configure for use with a CLI tool.
    Cli,
}

/// A builder that can be used to configure metrics reporting.
#[derive(Debug)]
pub struct Builder {
    /// The mode that our recorder should run in.
    mode: Mode,

    /// Global labels to apply to all metrics
    global_labels: HashMap<String, String>,
}

impl Builder {
    /// Create a builder than can be used to configure metrics reporting.
    pub fn new(mode: Mode) -> Builder {
        Builder {
            mode,
            global_labels: HashMap::new(),
        }
    }

    /// Add a label which will be attached to all metrics by default.
    pub fn add_global_label<K, V>(mut self, key: K, value: V) -> Self
    where
        K: Into<String>,
        V: Into<String>,
    {
        self.global_labels.insert(key.into(), value.into());
        self
    }

    /// Install an appropriate global metrics reporter, and return a handle to
    /// it.
    pub fn install(self) -> Result<Handle, Error> {
        if let Ok(new_relic_api_key) = env::var("NEW_RELIC_API_KEY") {
            info!("Reporting metrics to NewRelic");
            let mut builder = NewRelicBuilder::new(new_relic_api_key);
            for (key, value) in self.global_labels {
                builder = builder.add_global_label(key, value);
            }
            let handle = builder
                .build()
                .map_err(Error::new)?
                .install()
                .map_err(Error::new)?;
            Ok(Handle {
                inner: InnerHandle::NewRelic(handle),
            })
        } else {
            match self.mode {
                Mode::Cli => {
                    let mut prometheus_builder = PrometheusBuilder::new();
                    for (key, value) in self.global_labels {
                        prometheus_builder =
                            prometheus_builder.add_global_label(key, value);
                    }
                    let handle =
                        prometheus_builder.install_recorder().map_err(Error::new)?;
                    Ok(Handle {
                        inner: InnerHandle::Prometheus(handle),
                    })
                }
            }
        }
    }
}

/// Initialize an appropriate metrics registry, configured for a CLI tool.
///
/// Returns a handle that can be used to interact with the metrics reporter.
pub fn initialize_cli() -> Result<Handle, Error> {
    Builder::new(Mode::Cli).install()
}

/// An implementation-specific handle wrapper.
enum InnerHandle {
    NewRelic(NewRelicHandle),
    Prometheus(PrometheusHandle),
}

/// A handle that can be used to interact with the metrics registry.
pub struct Handle {
    /// We make this a member variable so we can hide it.
    inner: InnerHandle,
}

impl Handle {
    /// Report metrics using the chosen reporter.
    pub async fn report(&self) -> Result<(), Error> {
        match &self.inner {
            InnerHandle::NewRelic(handle) => {
                handle.report().await.map_err(Error::new)?
            }
            InnerHandle::Prometheus(handle) => info!("Metrics:\n{}", handle.render()),
        }
        Ok(())
    }
}
