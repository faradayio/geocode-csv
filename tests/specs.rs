//! Specifying columns to geocode.

use cli_test_dir::*;

/// A CSV file to geocode. Contains the empire state building.
const SIMPLE_CSV: &str = "address_1,address_2,city,state,zip_code
20 W 34th St,,New York,NY,10118
1224 S 760 W,,Provo,UT,
";

#[test]
#[ignore]
fn all_fields() {
    let testdir = TestDir::new("geocode-csv", "all_fields");

    testdir.create_file(
        "spec.json",
        r#"{
    "gc": {
        "house_number_and_street": [
            "address_1",
            "address_2"
        ],
        "city": "city",
        "state": "state",
        "postcode": "zip_code"
    }
}"#,
    );
    let bigtable_cache_url = std::env::var("BIGTABLE_CACHE_URL")
        .expect("BIGTABLE_CACHE_URL environment variable must be set");
    let output = testdir
        .cmd()
        .arg("--license=us-core-enterprise-cloud")
        .arg("--spec=spec.json")
        .arg(format!("--cache={}", bigtable_cache_url))
        .arg("--bigtable-random-eviction-age=1")
        .arg("--bigtable-random-eviction-rate=1.0")
        .output_with_stdin(SIMPLE_CSV)
        .expect_success();
    assert!(output.stdout_str().contains("gc_addressee"));
    assert!(output.stdout_str().contains("Commercial"));
    assert!(output.stdout_str().contains("Residential"));
    assert!(output.stdout_str().contains("40.21"));
}

// I can't find a license to run this test case right now.
//
// #[test]
// #[ignore]
// fn rooftop() {
//     let testdir = TestDir::new("geocode-csv", "rooftop");

//     testdir.create_file(
//         "spec.json",
//         r#"{
//     "gc": {
//         "house_number_and_street": [
//             "address_1",
//             "address_2"
//         ],
//         "city": "city",
//         "state": "state",
//         "postcode": "zip_code"
//     }
// }"#,
//     );
//     let output = testdir
//         .cmd()
//         .arg("--license=us-rooftop-geocoding-enterprise-cloud")
//         .arg("--spec=spec.json")
//         .output_with_stdin(SIMPLE_CSV)
//         .expect_success();
//     assert!(output.stdout_str().contains("gc_addressee"));
//     assert!(output.stdout_str().contains("40.217266"));
// }

#[test]
#[ignore]
fn single_address_field() {
    let testdir = TestDir::new("geocode-csv", "single_address_field");

    testdir.create_file(
        "spec.json",
        r#"{
    "gc": {
        "house_number_and_street": "address_1",
        "city": "city",
        "state": "state",
        "postcode": "zip_code"
    }
}"#,
    );

    let output = testdir
        .cmd()
        .arg("--license=us-core-enterprise-cloud")
        .arg("--spec=spec.json")
        .output_with_stdin(SIMPLE_CSV)
        .expect_success();
    assert!(output.stdout_str().contains("gc_addressee"));
    assert!(output.stdout_str().contains("Commercial"));
}

#[test]
#[ignore]
fn no_city_or_state() {
    let testdir = TestDir::new("geocode-csv", "no_city_or_state");

    testdir.create_file(
        "spec.json",
        r#"{
    "gc": {
        "house_number_and_street": [
            "address_1",
            "address_2"
        ],
        "postcode": "zip_code"
    }
}"#,
    );

    let output = testdir
        .cmd()
        .arg("--license=us-core-enterprise-cloud")
        .arg("--spec=spec.json")
        .output_with_stdin(SIMPLE_CSV)
        .expect_success();
    assert!(output.stdout_str().contains("gc_addressee"));
    assert!(output.stdout_str().contains("Commercial"));
}

#[test]
#[ignore]
fn freeform() {
    let testdir = TestDir::new("geocode-csv", "freeform");

    testdir.create_file(
        "spec.json",
        r#"{
    "gc": {
        "house_number_and_street": [
            "address_1",
            "address_2",
            "city",
            "state",
            "zip_code"
        ]
    }
}"#,
    );

    let output = testdir
        .cmd()
        .arg("--license=us-core-enterprise-cloud")
        .arg("--spec=spec.json")
        .output_with_stdin(SIMPLE_CSV)
        .expect_success();
    assert!(output.stdout_str().contains("gc_addressee"));
    assert!(output.stdout_str().contains("Commercial"));
}

#[test]
#[ignore]
fn multiple_addresses() {
    let testdir = TestDir::new("geocode-csv", "multiple_addresses");

    testdir.create_file(
        "spec.json",
        r#"{
    "shipping": {
        "house_number_and_street": [
            "address_1",
            "address_2"
        ],
        "postcode": "zip_code"
    },
    "billing": {
        "house_number_and_street": [
            "address_1",
            "address_2"
        ],
        "postcode": "zip_code"
    }
}"#,
    );

    let output = testdir
        .cmd()
        .arg("--license=us-core-enterprise-cloud")
        .arg("--spec=spec.json")
        .output_with_stdin(SIMPLE_CSV)
        .expect_success();
    assert!(output.stdout_str().contains("shipping_addressee"));
    assert!(output.stdout_str().contains("billing_addressee"));
}

#[test]
#[ignore]
fn rate_limiter() {
    let testdir = TestDir::new("geocode-csv", "rate_limiter");

    testdir.create_file(
        "spec.json",
        r#"{
    "shipping": {
        "house_number_and_street": [
            "address_1",
            "address_2"
        ],
        "postcode": "zip_code"
    }
}"#,
    );

    let output = testdir
        .cmd()
        .arg("--license=us-core-enterprise-cloud")
        .arg("--spec=spec.json")
        .arg("--max-addresses-per-second=300")
        .output_with_stdin(SIMPLE_CSV)
        .expect_success();
    assert!(output.stdout_str().contains("shipping_addressee"));
}

#[test]
#[ignore]
fn skip_records_with_empty_house_number_and_street() {
    let testdir = TestDir::new(
        "geocode-csv",
        "skip_records_with_empty_house_number_and_street",
    );

    testdir.create_file(
        "spec.json",
        r#"{
    "shipping": {
        "house_number_and_street": [
            "address_1",
            "address_2"
        ],
        "postcode": "zip_code"
    }
}"#,
    );

    let output = testdir
        .cmd()
        .arg("--license=us-core-enterprise-cloud")
        .arg("--spec=spec.json")
        .output_with_stdin(
            r#"address_1,address_2,city,state,zip_code
,,New York,NY,10118
 ,  ,Provo,UT,
"#,
        )
        .expect_success();
    // We output all lines, without geocoding any that lack a street address.
    assert!(output.stdout_str().contains("shipping_addressee"));
    assert!(output.stdout_str().contains("New York"));
    assert!(output.stdout_str().contains("Provo"));
}

#[test]
#[ignore]
fn append_libpostal() {
    let testdir = TestDir::new("geocode-csv", "append_libpostal");

    testdir.create_file(
        "spec.json",
        r#"{
    "gc": {
        "house_number_and_street": [
            "address_1",
            "address_2"
        ],
        "city": "city",
        "state": "state",
        "postcode": "zip_code"
    }
}"#,
    );
    let output = testdir
        .cmd()
        .arg("--license=us-core-enterprise-cloud")
        .arg("--spec=spec.json")
        .arg("--include-libpostal")
        .output_with_stdin(SIMPLE_CSV)
        .tee_output()
        .expect_success();
    assert!(output.stdout_str().contains("gc_addressee"));
    assert!(output.stdout_str().contains("Commercial"));
    assert!(output.stdout_str().contains("Residential"));
    assert!(output.stdout_str().contains("40.21"));
    assert!(output.stdout_str().contains("gc_libpostal_city"));
}

#[test]
#[ignore]
fn redis_cache_hit_test() {
    let testdir = TestDir::new("geocode-csv", "redis_cache_hit_test");

    testdir.create_file(
        "spec.json",
        r#"{
    "gc": {
        "house_number_and_street": [
            "address_1",
            "address_2"
        ],
        "city": "city",
        "state": "state",
        "postcode": "zip_code"
    }
}"#,
    );

    // First run - should call Smarty API and cache the result
    let output1 = testdir
        .cmd()
        .arg("--license=us-core-enterprise-cloud")
        .arg("--spec=spec.json")
        .arg("--cache=redis://localhost:6379")
        .output_with_stdin(SIMPLE_CSV)
        .expect_success();

    // Verify first run has correct data
    assert!(output1.stdout_str().contains("gc_addressee"));
    assert!(output1.stdout_str().contains("Commercial"));

    // Second run - should use cache, not call Smarty API
    let output2 = testdir
        .cmd()
        .arg("--license=us-core-enterprise-cloud")
        .arg("--spec=spec.json")
        .arg("--cache=redis://localhost:6379")
        .output_with_stdin(SIMPLE_CSV)
        .expect_success();

    // Verify second run has identical data (from cache)
    assert_eq!(output1.stdout_str(), output2.stdout_str());

    // Test cache-hits-only mode - should work without calling Smarty
    let output3 = testdir
        .cmd()
        .arg("--license=us-core-enterprise-cloud")
        .arg("--spec=spec.json")
        .arg("--cache=redis://localhost:6379")
        .arg("--cache-hits-only")
        .output_with_stdin(SIMPLE_CSV)
        .expect_success();

    // Should get the same cached data
    assert_eq!(output1.stdout_str(), output3.stdout_str());
}

#[test]
#[ignore]
fn bigtable_cache_hit_test() {
    let testdir = TestDir::new("geocode-csv", "bigtable_cache_hit_test");

    testdir.create_file(
        "spec.json",
        r#"{
    "gc": {
        "house_number_and_street": [
            "address_1",
            "address_2"
        ],
        "city": "city",
        "state": "state",
        "postcode": "zip_code"
    }
}"#,
    );

    // First run - should call Smarty API and cache the result
    let bigtable_cache_url = std::env::var("BIGTABLE_CACHE_URL")
        .expect("BIGTABLE_CACHE_URL environment variable must be set");
    let output1 = testdir
        .cmd()
        .arg("--license=us-core-enterprise-cloud")
        .arg("--spec=spec.json")
        .arg(format!("--cache={}", bigtable_cache_url))
        .output_with_stdin(SIMPLE_CSV)
        .expect_success();

    // Verify first run has correct data
    assert!(output1.stdout_str().contains("gc_addressee"));
    assert!(output1.stdout_str().contains("Commercial"));

    // Second run - should use cache, not call Smarty API
    let output2 = testdir
        .cmd()
        .arg("--license=us-core-enterprise-cloud")
        .arg("--spec=spec.json")
        .arg(format!("--cache={}", bigtable_cache_url))
        .output_with_stdin(SIMPLE_CSV)
        .expect_success();

    // Verify second run has identical data (from cache)
    assert_eq!(output1.stdout_str(), output2.stdout_str());

    // Test cache-hits-only mode - should work without calling Smarty
    let output3 = testdir
        .cmd()
        .arg("--license=us-core-enterprise-cloud")
        .arg("--spec=spec.json")
        .arg(format!("--cache={}", bigtable_cache_url))
        .arg("--cache-hits-only")
        .output_with_stdin(SIMPLE_CSV)
        .expect_success();

    // Should get the same cached data
    assert_eq!(output1.stdout_str(), output3.stdout_str());
}
