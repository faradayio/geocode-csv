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
    let output = testdir
        .cmd()
        .arg("--license=us-core-enterprise-cloud")
        .arg("--spec=spec.json")
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
