//! Specifying columns to geocode.

use cli_test_dir::*;

/// A CSV file to geocode. Contains the empire state building.
const SIMPLE_CSV: &str = "address_1,address_2,city,state,zip_code
20 W 34th St,,New York,NY,10118
1224 S 760 W,,Provo,UT,
104 16th st,,Belleair Bch,FL,
1600 Pennsylvania Ave NW,2nd Floor,Washington,DC,20500
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
    let stdout = output.stdout_str();
    assert!(stdout.contains("gc_city"));
    assert!(stdout.contains("new york"));
    assert!(stdout.contains("belleair"));

    // A secondary-unit designator like "2nd Floor" (a libpostal `level`) must
    // survive into our output instead of being silently dropped.
    assert!(stdout.contains("gc_level"));
    assert!(stdout.contains("floor"));

    // We should emit columns for the labels libpostal's parser can actually
    // produce, including ones we previously discarded.
    assert!(stdout.contains("gc_building"));
    assert!(stdout.contains("gc_metro_station"));

    // We should NOT emit columns for OpenCage formatting labels that libpostal's
    // parser never produces (it aliases these to canonical labels before
    // training).
    for dead_column in &[
        "gc_archipelago",
        "gc_continent",
        "gc_country_code",
        "gc_county",
        "gc_hamlet",
        "gc_municipality",
        "gc_neighbourhood",
        "gc_postal_city",
        "gc_region",
        "gc_village",
    ] {
        assert!(
            !stdout.contains(dead_column),
            "output should not contain dead column {dead_column}"
        );
    }
}
