//! Error-handling utilities.

use anyhow::Error;

/// Display an error, plus all the underlying "causes" (ie, wrapped errors), plus a
/// backtrace.
pub(crate) fn display_causes_and_backtrace(err: &Error) {
    eprintln!("Error: {}", err);
    for cause in err.chain().skip(1) {
        eprintln!("  caused by: {}", cause);
    }
    eprintln!("{}", err.backtrace());
}
