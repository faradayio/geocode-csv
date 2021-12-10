// Async HTTP boilerplate based on
// https://github.com/daboross/futures-example-2019/

#![recursion_limit = "128"]

use anyhow::Error;
pub use anyhow::Result;
use futures::FutureExt;
use std::{path::PathBuf, process::exit};
use structopt::StructOpt;

mod addresses;
mod async_util;
mod errors;
mod geocoder;
mod smartystreets;
mod structure;
mod unpack_vec;

use crate::addresses::AddressColumnSpec;
use crate::errors::display_causes_and_backtrace;
use crate::geocoder::{geocode_stdio, OnDuplicateColumns};
use crate::smartystreets::MatchStrategy;
use crate::structure::Structure;

/// Our command-line arguments.
#[derive(Debug, StructOpt)]
#[structopt(about = "geocode CSV files passed on standard input")]
struct Opt {
    /// `strict` for valid postal addresses only, `range` for unknown addresses
    /// within a street's known range, `invalid` to always generate some
    /// match, and `enhanced` if you've paid for it.
    #[structopt(long = "match", default_value = "strict")]
    match_strategy: MatchStrategy,

    /// What should we if geocoding output columns have the same names as input
    /// columns? [error, replace, append]
    #[structopt(long = "duplicate-columns", default_value = "error")]
    on_duplicate_columns: OnDuplicateColumns,

    /// A JSON file describing what columns to geocode.
    #[structopt(long = "spec")]
    spec_path: PathBuf,

    /// What license to use. Leave blank for standard, `us-rooftop-geocoding-enterprise-cloud` for Rooftop.
    #[structopt(long = "license", default_value = "us-standard-cloud")]
    license: String,
}

fn main() {
    // Set up basic logging.
    env_logger::init();

    if let Err(err) = run() {
        display_causes_and_backtrace(&err);
        exit(1);
    }
}

/// Our main entry point.
fn run() -> Result<()> {
    // Parse our command-line arguments.
    let opt = Opt::from_args();
    let spec = AddressColumnSpec::from_path(&opt.spec_path)?;
    let structure = Structure::complete()?;

    // Call our geocoder asynchronously.
    let geocode_fut = geocode_stdio(
        spec,
        opt.match_strategy,
        opt.license,
        opt.on_duplicate_columns,
        structure,
    );

    // Pass our future to our async runtime.
    let runtime = tokio::runtime::Runtime::new().expect("Unable to create a runtime");
    runtime.block_on(geocode_fut.boxed())?;
    Ok(())
}
