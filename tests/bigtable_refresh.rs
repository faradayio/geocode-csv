//! Integration tests for BigTable cache refresh.
//!
//! These require a real BigTable cache (`BIGTABLE_CACHE_URL`) and a Smarty
//! license, so they are `#[ignore]`d by default. The refresh decision itself
//! is unit-tested in `src/geocoders/cache/refresh.rs`.

use cli_test_dir::*;

const SIMPLE_CSV: &str = "address_1,address_2,city,state,zip_code
20 W 34th St,,New York,NY,10118
1 Infinite Loop,,Cupertino,CA,95014
";

const SPEC: &str = r#"{
    "gc": {
        "house_number_and_street": ["address_1", "address_2"],
        "city": "city",
        "state": "state",
        "postcode": "zip_code"
    }
}"#;

/// Fresh successes are younger than `--refresh-successes-after-days` (and
/// successes are not refreshed at all unless that flag is set). A second run
/// with refresh enabled should still return geocoded results from the cache.
#[test]
#[ignore]
fn fresh_entries_are_served_from_cache() {
    let testdir = TestDir::new("geocode-csv", "fresh_entries_are_served_from_cache");
    testdir.create_file("spec.json", SPEC);
    let bigtable_cache_url = std::env::var("BIGTABLE_CACHE_URL")
        .expect("BIGTABLE_CACHE_URL environment variable must be set");

    testdir
        .cmd()
        .arg("--license=us-core-enterprise-cloud")
        .arg("--spec=spec.json")
        .arg(format!("--cache={}", bigtable_cache_url))
        .output_with_stdin(SIMPLE_CSV)
        .expect_success();

    let output = testdir
        .cmd()
        .arg("--license=us-core-enterprise-cloud")
        .arg("--spec=spec.json")
        .arg(format!("--cache={}", bigtable_cache_url))
        .arg("--refresh-failures-after-days=90")
        .arg("--refresh-failures-max-attempts=4")
        .arg("--refresh-rate=1")
        .output_with_stdin(SIMPLE_CSV)
        .expect_success();

    assert!(output.stdout_str().contains("gc_addressee"));
}

/// Refresh only makes sense for the BigTable cache.
#[test]
fn refresh_requires_bigtable_cache() {
    let testdir = TestDir::new("geocode-csv", "refresh_requires_bigtable_cache");
    testdir.create_file("spec.json", SPEC);

    testdir
        .cmd()
        .arg("--spec=spec.json")
        .arg("--refresh-failures-after-days=90")
        .arg("--refresh-failures-max-attempts=4")
        .arg("--refresh-rate=1")
        .output_with_stdin(SIMPLE_CSV)
        .expect_failure();
}

#[test]
fn refresh_rate_nan_is_rejected() {
    let testdir = TestDir::new("geocode-csv", "refresh_rate_nan_is_rejected");
    testdir.create_file("spec.json", SPEC);

    testdir
        .cmd()
        .arg("--spec=spec.json")
        .arg("--cache=bigtable://project/instance/table")
        .arg("--refresh-failures-after-days=90")
        .arg("--refresh-failures-max-attempts=4")
        .arg("--refresh-rate=nan")
        .output_with_stdin(SIMPLE_CSV)
        .expect_failure();
}
