//! Specifying columns to geocode.

use cli_test_dir::*;

/// A CSV file to geocode. Contains the empire state building.
const SIMPLE_CSV: &str = "address_1,address_2,city,state,zip_code
20 W 34th St,,New York,NY,10118
1224 S 760 W,,Provo,UT,
104 16th st,,Belleair Bch,FL,
";

#[test]
#[ignore]
fn libpostal() {
    let testdir = TestDir::new("geocode-csv", "libpostal");

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
        .arg("--geocoder=libpostal")
        .arg("--spec=spec.json")
        .output_with_stdin(SIMPLE_CSV)
        .expect_success();
    assert!(output.stdout_str().contains("gc_city"));
    assert!(output.stdout_str().contains("new york"));
    assert!(output.stdout_str().contains("belleair"));
}
