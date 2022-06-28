//! Error-handling utilities.

use anyhow::Error;

/// Display an error, plus all the underlying "causes" (ie, wrapped errors), plus a
/// backtrace.
pub(crate) fn display_causes_and_backtrace(err: &Error) {
    eprintln!("{:?}", err);
}

/// Given a [`hyper::Error`], return a human-readable description.
///
/// This description _should_ be "low-arity", i.e., limited to only a handful of
/// values. We use it when reporting error metrics.
pub(crate) fn hyper_error_description_for_metrics(err: &hyper::Error) -> String {
    // HACK: We know that the current version of `hyper` puts a low-arity
    // description before ":", but follows it up with variable text.
    let display_err = err.to_string();
    let cut_at = display_err.find(':').unwrap_or(display_err.len());
    display_err[..cut_at].to_owned()
}
