use async_trait::async_trait;
use metrics::{counter, describe_counter};

use crate::{addresses::Address, Result};

use self::{
    client::{AddressRequest, SmartyClient},
    structure::Structure,
};

use super::{Geocoded, Geocoder, MatchStrategy, SharedHttpClient};

pub mod client;
mod structure;

/// Geocoding interface for Smarty.
pub struct Smarty {
    /// Our serialized configuration, in a format which can be used as a key.
    configuration_key: String,

    /// The names of the geocoding output columns we produce.
    column_names: Vec<String>,

    /// How should we match addresses?
    match_strategy: MatchStrategy,

    /// What Smarty license are we using?
    license: String,

    /// The structure of a Smarty response.
    structure: Structure,

    /// Our Smarty API client.
    client: SmartyClient,
}

impl Smarty {
    pub fn new(
        match_strategy: MatchStrategy,
        license: String,
        http_client: SharedHttpClient,
    ) -> Result<Smarty> {
        describe_counter!("geocodecsv.addresses_geocoded.total", "Addresses geocoded");

        let configuration_key = format!("{}:{}", match_strategy, license);
        let structure = Structure::complete()?;
        let column_names = structure.output_column_names()?;
        let client = SmartyClient::new(http_client)?;
        Ok(Smarty {
            configuration_key,
            column_names,
            match_strategy,
            license,
            structure,
            client,
        })
    }
}

#[async_trait]
impl Geocoder for Smarty {
    fn tag(&self) -> &str {
        "sm"
    }

    fn configuration_key(&self) -> &str {
        &self.configuration_key
    }

    fn column_names(&self) -> &[String] {
        &self.column_names
    }

    async fn geocode_addresses(
        &self,
        addresses: &[Address],
    ) -> Result<Vec<Option<Geocoded>>> {
        let requests = addresses
            .iter()
            .map(|addr| AddressRequest {
                match_strategy: self.match_strategy,
                address: addr.to_owned(), // This could be more efficient.
            })
            .collect::<Vec<_>>();

        let response = self
            .client
            .street_addresses(requests, self.license.to_owned())
            .await?;

        let hits = response.iter().filter(|g| g.is_some()).count();
        counter!("geocodecsv.addresses_geocoded.total", hits as u64, "geocoder" => "smarty", "geocode_result" => "found");
        counter!("geocodecsv.addresses_geocoded.total", (addresses.len() - hits) as u64, "geocoder" => "smarty", "geocode_result" => "unknown_address");

        // Build an array containing only the addresses that actually geocoded
        // successfully.
        let mut geocoded = vec![None; addresses.len()];
        for address_output in response.into_iter().flatten() {
            let column_values =
                self.structure.value_columns_for(&address_output.fields)?;
            geocoded[address_output.input_index] = Some(Geocoded { column_values });
        }

        Ok(geocoded)
    }
}