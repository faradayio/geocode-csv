//! Error-handling utilities.

use anyhow::Error;

/// Display an error, plus all the underlying "causes" (ie, wrapped errors), plus a
/// backtrace.
pub(crate) fn display_causes_and_backtrace(err: &Error) {
    eprintln!("{:?}", err);
}
