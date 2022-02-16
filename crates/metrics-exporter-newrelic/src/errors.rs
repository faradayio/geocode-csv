//! Error types.

use metrics::SetRecorderError;
use thiserror::Error;

/// An error occurred building or installing our metrics recorder.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum BuildError {
    #[error("Could not install NewRelic metrics recorder")]
    #[non_exhaustive]
    SetRecorderError {
        #[from]
        source: SetRecorderError,
    },
}

/// An error occurred building or installing our metrics recorder.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ReportError {
    /// Making a request to NewRelic failed.
    #[error("Could not make an HTTP request to NewRelic")]
    #[non_exhaustive]
    HttpError {
        source: Box<dyn std::error::Error + Send + Sync + 'static>,
    },

    /// NewRelic returned an unexpected HTTP status when reporting metrics.
    #[error("Unexpected NewRelic HTTP status {status} ({body})")]
    #[non_exhaustive]
    UnexpectedHttpStatus {
        /// The HTTP status code.
        status: http::StatusCode,
        /// The HTTP body returned by NewRelic.
        body: String,
    },
}

/// We could override `From::from` to implement these methods, but that would
/// add the conversions to our public API. And we don't want to admit we use
/// `reqwest`. So we use methods on `ReportError`, which we don't automatially
/// export the way we would a trait `impl`.
impl ReportError {
    pub(crate) fn from_error(source: reqwest::Error) -> Self {
        ReportError::HttpError {
            source: Box::new(source),
        }
    }

    pub(crate) async fn from_response(response: reqwest::Response) -> Self {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        ReportError::UnexpectedHttpStatus { status, body }
    }
}
