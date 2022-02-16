//! Error types.

use thiserror::Error;

/// A `libpostal`-related error.
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum Error {
    /// We could initialize libpostal for some reason.
    #[error("could not initialize libpostal ({component})")]
    #[non_exhaustive]
    InitializationFailed { component: &'static str },

    /// The `libpostal` data directory was not found in any of the usual
    /// locations.
    #[error("could not find libpostal data in any of {candidates:?}")]
    #[non_exhaustive]
    NoDataDir { candidates: &'static [&'static str] },

    /// We cannot pass strings with `\0` bytes to C.
    #[error("found a '\0' byte in {string:?}")]
    #[non_exhaustive]
    NullByteInString { string: String },
}
