//! Code to support server mode.

use std::collections::HashMap;
use std::iter::FromIterator;
use std::sync::Arc;

use crate::addresses::Address;
use crate::geocoders::Geocoded;
use crate::geocoders::Geocoder;
use anyhow::{format_err, Context, Result};
use axum::{
    extract::DefaultBodyLimit,
    headers::{HeaderMap, HeaderName},
    http::header::CONTENT_TYPE,
    http::StatusCode,
    routing::post,
    Extension, Json, Router,
};
use serde::{Deserialize, Serialize};

/// An error message to serialize as JSON on error.
#[derive(Serialize)]
struct ErrorResponse {
    /// A human-readable error.
    message: String,
}

impl ErrorResponse {
    /// Construct an error response from an error.
    fn new(err: anyhow::Error) -> Self {
        ErrorResponse {
            message: err.to_string(),
        }
    }
}

struct State {
    geocoder: Box<dyn Geocoder>,
}

// Run the server. Should not return.
pub async fn run_server(listen_addr: &str, geocoder: Box<dyn Geocoder>) -> Result<()> {
    // Build our application with a single route.
    let state = Arc::new(State { geocoder });

    let app = Router::new()
        .route("/geocode", post(handle_post_geocode))
        .layer(Extension(state))
        // Assumes ~128 addresses at ~128 bytes each. More than this would (1)
        // need to be streamed instead of read into memory, and (2) chunked
        // before passing to the geocoder layer which expects chunks in the
        // rough size of `[crate::pipeline::GEOCODE_SIZE]`.
        .layer(DefaultBodyLimit::max(16384));

    let listen_addr = listen_addr.parse().with_context(|| {
        format!("could not parse listen address: {:?}", listen_addr)
    })?;

    // Run it with axum on the given listen address.
    axum::Server::bind(&listen_addr)
        .serve(app.into_make_service())
        .await
        .context("web server failed to start")
}

/// Our /geocode request format.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GeocodeRequest {
    /// Addresses to geocode. These are _always_ a list because the underlying
    /// engine is much more efficient that way and we to encourage people to
    /// use it the right way.
    addresses: Vec<Address>,
}

/// Our geocode response format.
#[derive(Debug, Serialize)]
struct GeocodeResponse {
    /// The geocoder output. There is one record here for each input record, in
    /// the same order. `None` means we failed to find a match. `Some` returns
    /// key/value pairs that are dependent on the configured geocoder.
    results: Vec<Option<HashMap<String, String>>>,
}

/// POST /geocode
async fn handle_post_geocode(
    Extension(state): Extension<Arc<State>>,
    headers: HeaderMap,
    Json(body): Json<GeocodeRequest>,
) -> Result<(StatusCode, Json<GeocodeResponse>), (StatusCode, Json<ErrorResponse>)> {
    // Get our geocoder.
    let geocoder = state.as_ref().geocoder.as_ref();

    // Require users to specify this, so that we can later add JSON support
    // without breaking anything.
    if let Err(err) = expect_header_value(&headers, &CONTENT_TYPE, "application/json")
    {
        return Err((StatusCode::BAD_REQUEST, Json(ErrorResponse::new(err))));
    }

    let result = geocoder.geocode_addresses(&body.addresses).await;

    match result {
        Ok(geocoded) => {
            let column_names = geocoder.column_names();
            let response = GeocodeResponse {
                results: geocoded
                    .into_iter()
                    .map(|g: Option<Geocoded>| -> Option<HashMap<String, String>> {
                        g.map(|g| hash_from_geocoded(column_names, &g))
                    })
                    .collect(),
            };
            Ok((StatusCode::OK, Json(response)))
        }
        Err(err) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse::new(err)),
        )),
    }
}

fn hash_from_geocoded(
    column_names: &[String],
    geocoded: &Geocoded,
) -> HashMap<String, String> {
    HashMap::from_iter(
        column_names
            .iter()
            .cloned()
            .zip(geocoded.column_values.iter().cloned()),
    )
}

fn expect_header_value(
    headers: &HeaderMap,
    header_name: &HeaderName,
    expected: &str,
) -> Result<()> {
    match headers.get(CONTENT_TYPE) {
        Some(v) if v == expected => Ok(()),
        Some(v) => Err(format_err!(
            "expected {} {:?}, got {:?}",
            header_name,
            expected,
            v
        )),
        None => Err(format_err!("Missing header {}", header_name)),
    }
}
