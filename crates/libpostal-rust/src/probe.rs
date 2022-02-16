//! Probing for our data directory.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::RwLock,
};

use lazy_static::{__Deref, lazy_static};
use tracing::{debug, error, trace};

use crate::{Error, Result};

/// What data version of libpostal do we expect to find?
const EXPECTED_DATA_VERSION: &str = "v1";

/// Possible places we might find the libpostal data directories.
static DATA_DIR_PATHS: &[&str] =
    &["/usr/local/share/libpostal", "/usr/share/libpostal"];

lazy_static! {
    static ref DATA_DIRECTORY: RwLock<Option<PathBuf>> = RwLock::new(None);
}

/// Probe for a libpostal data directory, returning the path, if any.
pub(crate) fn probe_data_directory() -> Result<PathBuf> {
    // Return a saved probe value if we already have one.
    {
        let data_directory = DATA_DIRECTORY.read().expect("lock poisoned");
        if let Some(data_directory) = data_directory.deref().as_ref() {
            return Ok(data_directory.to_owned());
        }
    }

    debug!("probing for libpostal data directory");
    for &path in DATA_DIR_PATHS {
        let path = Path::new(path);
        let data_version_path = path.join("data_version");
        if data_version_path.exists() {
            match fs::read_to_string(&data_version_path) {
                Ok(version) => {
                    if version.trim_end() == EXPECTED_DATA_VERSION {
                        debug!("using {} as libpostal data directory", path.display());
                        let mut data_directory =
                            DATA_DIRECTORY.write().expect("lock poisoned");
                        *data_directory = Some(path.to_owned());
                        return Ok(path.to_owned());
                    } else {
                        trace!(
                            "skipping {} because data_version is not {}",
                            path.display(),
                            EXPECTED_DATA_VERSION
                        );
                    }
                }
                Err(err) => {
                    error!(
                        "could not read {} (skipping): {}",
                        data_version_path.display(),
                        err
                    );
                }
            }
        } else {
            trace!("no {}, skipping", data_version_path.display());
        }
    }

    debug!("Could not find libpostal data directory");
    Err(Error::NoDataDir {
        candidates: DATA_DIR_PATHS,
    })
}
