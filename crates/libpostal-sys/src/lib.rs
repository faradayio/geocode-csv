#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
// This is pretty bad, but we'll just avoid using those APIs, I hope?
#![allow(improper_ctypes)]
// Ignore test-only warnings caused by
// https://github.com/rust-lang/rust-bindgen/issues/1651.
#![cfg_attr(test, allow(deref_nullptr))]

use std::sync::{Arc, Mutex};

use lazy_static::lazy_static;

/// Which portions of this library have been initialized?
///
/// A global copy of this struct is protected by our `GLOBAL_LOCK`.
#[derive(Debug)]
#[non_exhaustive]
pub struct InitializationState {
    /// Has the main library been initialized?
    pub initialized: bool,

    /// Have we initialized the address parser?
    pub parser_initialized: bool,

    /// Have we initialized the language classifier?
    pub language_classifier_initialized: bool,
}

lazy_static! {
    /// You _must_ take this lock before doing _anything_ with this library.
    /// `libpostal` is [not thread safe][threads].
    ///
    /// [threads]: https://github.com/openvenues/libpostal/issues/34
    pub static ref GLOBAL_LOCK: Arc<Mutex<InitializationState>> = Arc::new(Mutex::new(InitializationState {
        initialized: false,
        parser_initialized: false,
        language_classifier_initialized: false,
    }));
}

// We could use build.rs to automatically generate these bindings, then include
//them like this. include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

// Assume our bindings were generated using:
//
//     bindgen wrapper.h -o src/bindings.rs
//
// See the README.md file.
include!("bindings.rs");

#[cfg(test)]
mod tests {
    use std::ffi::{CStr, CString};

    use super::*;

    #[test]
    #[ignore]
    fn smoke_test() {
        unsafe {
            let datadir = CString::new("/usr/local/share/libpostal")
                .expect("CString::new failed");
            if !libpostal_setup_datadir(datadir.as_ptr() as *mut _) {
                panic!("can't set up libpostal");
            }
            if !libpostal_setup_parser_datadir(datadir.as_ptr() as *mut _) {
                panic!("can't set up libpostal parser")
            }
            if !libpostal_setup_language_classifier_datadir(datadir.as_ptr() as *mut _)
            {
                panic!("can't set up libpostal classifier")
            }

            // Parse an address and print components.
            let address = CString::new(
                "781 Franklin Ave Crown Heights Brooklyn NYC NY 11216 USA",
            )
            .expect("CString::new failed");
            let parse_options = libpostal_get_address_parser_default_options();
            let parsed =
                libpostal_parse_address(address.as_ptr() as *mut _, parse_options);
            for i in 0..(*parsed).num_components {
                let label = CStr::from_ptr(*(*parsed).labels.offset(i as isize))
                    .to_str()
                    .expect("label contained invalid UTF-8");
                let component =
                    CStr::from_ptr(*(*parsed).components.offset(i as isize))
                        .to_str()
                        .expect("component contained invalid UTF-8");
                println!("{}={}", label, component);
            }
            libpostal_address_parser_response_destroy(parsed);

            // Normalize a (street?) address.
            let street_address =
                CString::new("Quatre-vingt-douze Ave des Champs-Élysées")
                    .expect("CString::new failed");
            let normalization_options = libpostal_get_default_options();
            let mut num_expansions: size_t = 0;
            let expansions = libpostal_expand_address(
                street_address.as_ptr() as *mut _,
                normalization_options,
                &mut num_expansions,
            );
            for i in 0..num_expansions {
                let expansion = CStr::from_ptr(*expansions.offset(i as isize))
                    .to_str()
                    .expect("expansion contained invalid UTF-8");
                println!("expansion {}: {}", i, expansion);
            }
            libpostal_expansion_array_destroy(expansions, num_expansions);

            // Clean up the library.
            libpostal_teardown();
            libpostal_teardown_parser();
            libpostal_teardown_language_classifier();
        }
    }
}
