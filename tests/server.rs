//! Test server mode.

use std::thread::sleep;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use cli_test_dir::*;
use hyper::header::CONTENT_TYPE;
use reqwest::blocking::{Client, Response};
use serde::Serialize;

#[derive(Serialize)]
struct AddressJson {
    street: &'static str,
    city: Option<&'static str>,
    state: Option<&'static str>,
    zipcode: Option<&'static str>,
}
#[derive(Serialize)]
struct AddressesJson {
    addresses: Vec<AddressJson>,
}

#[test]
#[ignore]
fn server() -> Result<()> {
    let testdir = TestDir::new("geocode-csv", "");

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
    let mut child = testdir
        .cmd()
        .arg("--geocoder=libpostal")
        .arg("--spec=spec.json")
        .arg("server")
        .spawn()
        .context("server failed to start")?;

    // Call our helper to actually make the HTTP request, clean up our webserver
    // (always!), and check to see if `result` was an error.
    let result = server_helper();
    if let Err(err) = child.kill() {
        eprintln!("could not stop web server: {}", err);
    }
    let response = result?;

    // Receive CSV and check that it contains the correct content.
    if !response.status().is_success() {
        let status = response.status();
        let body = match response.text() {
            Ok(body) => body,
            Err(err) => err.to_string(),
        };
        panic!("error status from server: {:?}\nbody: {}", status, body);
    }
    let output = response.text().context("couldn't get response body")?;

    eprintln!("output from request:\n{}", output);
    assert!(output.contains("road"));
    assert!(output.contains("w 34th st"));
    Ok(())
}

/// Helper function for `server` test, so that test can clean up the actual
/// server process. This must _not_ use `assert!` or other functions that panic,
/// or we won't clean up.
fn server_helper() -> Result<Response> {
    // Create an HTTP client.
    let client = Client::new();

    // Addresses to geocode.
    let addresses = AddressesJson {
        addresses: vec![
            AddressJson {
                street: "20 W 34th St",
                city: Some("New York"),
                state: Some("NY"),
                zipcode: Some("10118"),
            },
            AddressJson {
                street: "1224 S 760 W",
                city: Some("Provo"),
                state: Some("UT"),
                zipcode: None,
            },
        ],
    };

    // We may need to retry this several times depending on how long the server
    // takes to start.
    let connect = || {
        client
            .post("http://localhost:8787/geocode")
            .header(CONTENT_TYPE, "application/json")
            .json(&addresses)
            .send()
            .context("HTTP request failed")
    };

    // Post a request with JSON content.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match connect() {
            Ok(res) => break Ok(res),
            Err(err) if Instant::now() < deadline => {
                eprintln!("request failed; retrying: {:?}", err);
                sleep(Duration::from_millis(100));
            }
            Err(err) => return Err(err).context("http request timed out with error"),
        }
    }
}
