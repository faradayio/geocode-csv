//! Interface to Smarty REST API.

use std::time::Instant;
use std::{env, str};

use anyhow::{format_err, Context};
use metrics::{counter, describe_histogram, histogram, Unit};
use serde::{Deserialize, Serialize};
use tracing::{error, instrument};
use url::Url;

use crate::addresses::Address;
use crate::geocoders::MatchStrategy;
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

pub struct SmartyClient {
    credentials: Credentials,
    client: reqwest::Client,
}

impl SmartyClient {
    pub fn new(client: reqwest::Client) -> Result<SmartyClient> {
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
        let start = Instant::now();

        let mut url = Url::parse("https://api.smartystreets.com/street-address")?;
        url.query_pairs_mut()
            .append_pair("auth-id", &self.credentials.auth_id)
            .append_pair("auth-token", &self.credentials.auth_token)
            .append_pair("license", &license)
            .finish();

        let res = match self.client.post(url.as_str()).json(&requests).send().await {
            Ok(res) => res,
            Err(err) => {
                let desc = reqwest_error_description_for_metrics(&err);
                counter!("geocodecsv.selected_errors.count", "component" => "smarty", "cause" => desc).increment(1);
                return Err(err).context("smarty request failed");
            }
        };
        let status = res.status();
        let body_data = res
            .bytes()
            .await
            .context("failed to read smarty response body")?;

        histogram!("geocodecsv.smarty.geocode_request.duration_seconds")
            .record((Instant::now() - start).as_secs_f64());

        if status.is_success() {
            let resps: Vec<AddressResponse> = serde_json::from_slice(&body_data)?;
            Ok(unpack_vec(resps, requests.len(), |resp| resp.input_index)?)
        } else {
            counter!("geocodecsv.selected_errors.count", "component" => "smarty", "cause" => status.to_string()).increment(1);

            if status == 422 {
                if let Ok(error_response) =
                    serde_json::from_slice::<SmartyErrorResponse>(&body_data)
                {
                    if error_response
                        .errors
                        .iter()
                        .any(|e| e.name == "us-street-api:query-missing-street")
                    {
                        let streets = requests
                            .iter()
                            .map(|req| req.address.street.to_owned())
                            .collect::<Vec<_>>();
                        error!("At least one missing street in: {:?}", streets);
                    }
                }
            }

            Err(format_err!(
                "geocoding error: {}\n{}",
                status,
                String::from_utf8_lossy(&body_data),
            ))
        }
    }
}

fn reqwest_error_description_for_metrics(err: &reqwest::Error) -> &'static str {
    if err.is_timeout() {
        "timeout"
    } else if err.is_connect() {
        "connect"
    } else if err.is_request() {
        "request"
    } else {
        "other"
    }
}

/// Smarty error response body.
#[derive(Debug, Deserialize)]
struct SmartyErrorResponse {
    errors: Vec<SmartyError>,
}

/// Smarty error.
#[derive(Debug, Deserialize)]
struct SmartyError {
    name: String,
}
