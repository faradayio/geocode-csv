//! Interface to Smarty REST API.

use std::time::Instant;
use std::{env, str};

use anyhow::{format_err, Context};
use futures::stream::StreamExt;
use hyper::{Body, Request};
use metrics::{counter, describe_histogram, histogram, Unit};
use serde::{Deserialize, Serialize};
use tracing::instrument;
use url::Url;

use crate::addresses::Address;
use crate::errors::hyper_error_description_for_metrics;
use crate::geocoders::{MatchStrategy, SharedHttpClient};
use crate::unpack_vec::unpack_vec;
use crate::Result;

/// Credentials for authenticating with Smarty.
#[derive(Debug, Clone)]
pub struct Credentials {
    auth_id: String,
    auth_token: String,
}

impl Credentials {
    /// Create new Smarty credentials from environment variables.
    fn from_env() -> Result<Credentials> {
        let auth_id = env::var("SMARTY_AUTH_ID")
            .or_else(|_| env::var("SMARTYSTREETS_AUTH_ID"))
            .context("could not read SMARTY_AUTH_ID")?;
        let auth_token = env::var("SMARTY_AUTH_TOKEN")
            .or_else(|_| env::var("SMARTYSTREETS_AUTH_TOKEN"))
            .context("could not read SMARTY_AUTH_TOKEN")?;
        Ok(Credentials {
            auth_id,
            auth_token,
        })
    }
}

/// A Smarty address request.
#[derive(Clone, Debug, Serialize)]
pub struct AddressRequest {
    /// The address to geocode.
    #[serde(flatten)]
    pub address: Address,

    /// What match strategy should we use?
    #[serde(rename = "match")]
    pub match_strategy: MatchStrategy,
}

/// A Smarty address response.
#[derive(Clone, Debug, Deserialize)]
pub struct AddressResponse {
    /// The index of the corresponding `AddressRequest`.
    pub input_index: usize,

    /// Fields returned by Smarty. We could actually represent this as
    /// serveral large structs with known fields, and it would probably be
    /// faster, but this way requires less code for now.
    #[serde(flatten)]
    pub fields: serde_json::Value,
}

/// The real implementation of `S.
pub struct SmartyClient {
    credentials: Credentials,
    client: SharedHttpClient,
}

impl SmartyClient {
    /// Create a new Smarty client.
    pub fn new(client: SharedHttpClient) -> Result<SmartyClient> {
        describe_histogram!(
            "geocodecsv.smart.geocode_request.duration_seconds",
            Unit::Seconds,
            "Time required for Smarty to geocode a batch of rows"
        );

        Ok(SmartyClient {
            credentials: Credentials::from_env()?,
            client,
        })
    }

    /// Geocode addresses using Smarty.
    #[instrument(
        name = "SmartyClient::street_addresses",
        level="debug",
        skip_all,
        fields(addresses.len = requests.len())
    )]
    pub async fn street_addresses(
        &self,
        requests: Vec<AddressRequest>,
        license: String,
    ) -> Result<Vec<Option<AddressResponse>>> {
        street_addresses_impl(
            self.credentials.clone(),
            self.client.clone(),
            requests,
            license,
        )
        .await
    }
}

/// The real implementation of `street_addresses`.
async fn street_addresses_impl(
    credentials: Credentials,
    client: SharedHttpClient,
    requests: Vec<AddressRequest>,
    license: String,
) -> Result<Vec<Option<AddressResponse>>> {
    let start = Instant::now();

    // Build our URL.
    let mut url = Url::parse("https://api.smartystreets.com/street-address")?;
    url.query_pairs_mut()
        .append_pair("auth-id", &credentials.auth_id)
        .append_pair("auth-token", &credentials.auth_token)
        .append_pair("license", &license)
        .finish();

    // Make the geocoding request.
    let req = Request::builder()
        .method("POST")
        .uri(url.as_str())
        .header("Content-Type", "application/json; charset=utf-8")
        .body(Body::from(serde_json::to_string(&requests)?))?;
    let res = match client.request(req).await {
        Ok(res) => res,
        Err(err) => {
            // Errors that occur here are being reported by our local HTTP
            // stack, not the remote server.
            let desc = hyper_error_description_for_metrics(&err);
            counter!("geocodecsv.selected_errors.count", 1, "component" => "smarty", "cause" => desc);
            return Err(err.into());
        }
    };
    let status = res.status();
    let mut body = res.into_body();
    let mut body_data = vec![];
    while let Some(chunk_result) = body.next().await {
        let chunk = chunk_result?;
        body_data.extend(&chunk[..]);
    }

    histogram!(
        "geocodecsv.smarty.geocode_request.duration_seconds",
        (Instant::now() - start).as_secs_f64(),
    );

    // Check the request status.
    if status.is_success() {
        let resps: Vec<AddressResponse> = serde_json::from_slice(&body_data)?;
        Ok(unpack_vec(resps, requests.len(), |resp| resp.input_index)?)
    } else {
        // This error was reported by the remote server.
        counter!("geocodecsv.selected_errors.count", 1, "component" => "smarty", "cause" => status.to_string());
        Err(format_err!(
            "geocoding error: {}\n{}",
            status,
            String::from_utf8_lossy(&body_data),
        ))
    }
}
