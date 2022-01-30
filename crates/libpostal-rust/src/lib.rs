//! `libpostal-rust`: A high-level, threadsafe wrapper around `libpostal`
//!
//! The open source C library
//! [`libpostal`](https://github.com/openvenues/libpostal) provides support for
//! parsing and normalizing addresses using an external language model trained
//! on addresses around the world. We provide a high-level Rust wrapper around
//! that library, in a way that it can be linked into your main Rust binary.
//!
//! Note that you will need to use `libpostal_data` (included with `libpostal`)
//! to download and install about 2GB of language model data:
//!
//! ```sh
//! sudo libpostal_data download all /usr/local/share/libpostal
//! ```
//!
//! Once this is done, you can parse addresses as follows:
//!
//! ```no_run
//! use libpostal_rust::{ParseAddressOptions, parse_address};
//!
//! let addr = "781 Franklin Ave Crown Heights Brooklyn NYC NY 11216 USA";
//! let opt = ParseAddressOptions::default();
//! let parsed = parse_address(addr, &opt).unwrap();
//! assert_eq!(parsed.get("state"), Some(&"ny".to_owned()));
//! ```
//!
//! You can turn `parsed` back into a nicely-formatted address (almost anywhere
//! in the world) by using
//! [`address-formatter`](https://crates.io/crates/address-formatter)'s support
//! for OpenCage address templates.

use std::{
    collections::HashMap,
    ffi::{CStr, CString},
    ops::DerefMut,
};

use init::{
    initialize_libpostal, initialize_libpostal_language_classifier,
    initialize_libpostal_parser,
};
use libpostal_sys::{
    libpostal_address_parser_response_destroy, libpostal_expand_address,
    libpostal_expansion_array_destroy, libpostal_get_address_parser_default_options,
    libpostal_get_default_options, libpostal_parse_address, size_t, GLOBAL_LOCK,
};

mod errors;
mod init;
mod probe;

pub use self::errors::Error;

/// A `Result` type which defaults to `libpostal_rust::Error`.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Options for use with `parse_address`.
///
/// Right now, this is just a placeholder and you can't set any options yet.
#[derive(Debug, Default)]
pub struct ParseAddressOptions {}

/// Parse an address into its component values.
pub fn parse_address(
    addr: &str,
    _opt: &ParseAddressOptions,
) -> Result<HashMap<String, String>> {
    // We need to hold onto this lock whenever we're calling libpostal.
    let mut initialization_state = GLOBAL_LOCK.lock().expect("mutex poisoned");
    unsafe { initialize_libpostal(initialization_state.deref_mut()) }?;
    unsafe { initialize_libpostal_parser(initialization_state.deref_mut()) }?;

    // Convert our arguments to work with C.
    let addr = CString::new(addr).map_err(|_| Error::NullByteInString {
        string: addr.to_owned(),
    })?;
    let parse_options = unsafe { libpostal_get_address_parser_default_options() };

    // Parse the address.
    let parsed =
        unsafe { libpostal_parse_address(addr.as_ptr() as *mut _, parse_options) };

    // Convert `parsed` to a reasonable Rust value.
    let num_components = unsafe { (*parsed).num_components } as usize;
    let mut result = HashMap::with_capacity(num_components);
    for i in 0..num_components {
        let (label, component) = unsafe {
            (
                CStr::from_ptr(*(*parsed).labels.add(i))
                    .to_str()
                    .expect("label contained invalid UTF-8"),
                CStr::from_ptr(*(*parsed).components.add(i))
                    .to_str()
                    .expect("component contained invalid UTF-8"),
            )
        };
        result.insert(label.to_owned(), component.to_owned());
    }

    // Clean up our C data structure.
    unsafe { libpostal_address_parser_response_destroy(parsed) };

    Ok(result)
}

/// Options for use with `expand_address`.
#[derive(Debug, Default)]
pub struct ExpandAddressOptions {}

/// Try to expand any abbreviations in an address.
pub fn expand_address(addr: &str, _opt: &ExpandAddressOptions) -> Result<Vec<String>> {
    // We need to hold onto this lock whenever we're calling libpostal.
    let mut initialization_state = GLOBAL_LOCK.lock().expect("mutex poisoned");
    unsafe { initialize_libpostal(initialization_state.deref_mut()) }?;
    unsafe {
        initialize_libpostal_language_classifier(initialization_state.deref_mut())
    }?;

    // Convert our arguments to work with C.
    let addr = CString::new(addr).map_err(|_| Error::NullByteInString {
        string: addr.to_owned(),
    })?;
    let expand_options = unsafe { libpostal_get_default_options() };

    // Parse the address.
    let mut num_expansions: size_t = 0;
    let expansions = unsafe {
        libpostal_expand_address(
            addr.as_ptr() as *mut _,
            expand_options,
            &mut num_expansions,
        )
    };

    // Convert our results for Rust.
    let mut result = Vec::with_capacity(num_expansions as usize);
    for i in 0..num_expansions {
        let expansion = unsafe {
            CStr::from_ptr(*expansions.offset(i as isize))
                .to_str()
                .expect("expansion contained invalid UTF-8")
        };
        result.push(expansion.to_owned());
    }

    // Clean up our C data structure.
    unsafe { libpostal_expansion_array_destroy(expansions, num_expansions) };

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore]
    fn parse_address_returns_components() {
        let addr = "781 Franklin Ave Crown Heights Brooklyn NYC NY 11216 USA";
        let opt = ParseAddressOptions::default();
        let parsed = parse_address(addr, &opt).unwrap();
        assert_eq!(parsed.get("state"), Some(&"ny".to_owned()));
    }

    #[test]
    #[ignore]
    fn expand_address_returns_candidates() {
        let addr = "Quatre-vingt-douze Ave des Champs-Élysées";
        let opt = ExpandAddressOptions::default();
        let expanded = expand_address(addr, &opt).unwrap();
        assert!(expanded[0].contains("92"));
    }
}
