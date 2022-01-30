use std::{
    ffi::{CStr, CString},
    os::unix::ffi::OsStrExt,
    path::Path,
};

use libpostal_sys::{
    libpostal_setup_datadir, libpostal_setup_language_classifier_datadir,
    libpostal_setup_parser_datadir, InitializationState,
};
use tracing::debug_span;

use crate::{probe::probe_data_directory, Error, Result};

/// Convert a path to a C string. This is portable right up until someone uses
/// weird characters in a Windows file name or strange encodings.
fn path_as_c_string(path: &Path) -> Result<CString> {
    let mut bytes = path.as_os_str().as_bytes().to_owned();
    bytes.push(b'\0');
    let c_str =
        CStr::from_bytes_with_nul(&bytes).map_err(|_| Error::NullByteInString {
            string: path.display().to_string(),
        })?;
    Ok(c_str.to_owned())
}

///! Support for initializing `libpostal`.
pub(crate) unsafe fn initialize_libpostal(
    state: &mut InitializationState,
) -> Result<()> {
    if !state.initialized {
        let _span = debug_span!("libpostal_setup_datadir").entered();
        let datadir = probe_data_directory()?;
        let datadir_c_string = path_as_c_string(&datadir)?;
        if !libpostal_setup_datadir(datadir_c_string.as_ptr() as *mut i8) {
            return Err(Error::InitializationFailed { component: "base" });
        }
        state.initialized = true;
    }
    Ok(())
}

///! Support for initializing `libpostal`'s parser module.
pub(crate) unsafe fn initialize_libpostal_parser(
    state: &mut InitializationState,
) -> Result<()> {
    if !state.parser_initialized {
        let _span = debug_span!("libpostal_setup_parser_datadir").entered();
        let datadir = probe_data_directory()?;
        let datadir_c_string = path_as_c_string(&datadir)?;
        if !libpostal_setup_parser_datadir(datadir_c_string.as_ptr() as *mut i8) {
            return Err(Error::InitializationFailed {
                component: "parser",
            });
        }
        state.parser_initialized = true;
    }
    Ok(())
}

///! Support for initializing `libpostal`'s parser module.
pub(crate) unsafe fn initialize_libpostal_language_classifier(
    state: &mut InitializationState,
) -> Result<()> {
    if !state.language_classifier_initialized {
        let _span =
            debug_span!("libpostal_setup_language_classifier_datadir").entered();
        let datadir = probe_data_directory()?;
        let datadir_c_string = path_as_c_string(&datadir)?;
        if !libpostal_setup_language_classifier_datadir(
            datadir_c_string.as_ptr() as *mut i8
        ) {
            return Err(Error::InitializationFailed {
                component: "language classifier",
            });
        }
        state.language_classifier_initialized = true;
    }
    Ok(())
}
