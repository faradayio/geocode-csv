//! "Geocoding" interface based on `libpostal`. This really just does address
//! normalization, and doesn't geocode.

use async_trait::async_trait;
use libpostal_rust::{parse_address, ParseAddressOptions};
use metrics::{counter, describe_counter};

use crate::{addresses::Address, Result};

use super::{Geocoded, Geocoder};

pub(crate) static COLUMN_NAMES: &[&str] = &[
    // These are the address-component labels that libpostal's address parser can
    // actually emit, as defined in
    // `crates/libpostal-sys/libpostal/src/address_parser.h`. Earlier versions of
    // this list were copied from OpenCage's address-_formatting_ vocabulary
    // (components.yaml), which is a different vocabulary: it includes names like
    // `county`, `neighbourhood`, and `hamlet` that libpostal's parser never
    // produces (it aliases those to `state_district`, `suburb`, and `city`
    // respectively before training). We omit the non-address parser labels
    // `category`, `near`, `website`, and `phone`.
    "building",
    "city",
    "city_district",
    "country",
    "country_region",
    "entrance",
    "house",
    "house_number",
    "island",
    "level",
    "metro_station",
    "po_box",
    "postcode",
    "road",
    "staircase",
    "state",
    "state_district",
    "suburb",
    "unit",
    "world_region",
];

pub struct LibPostal {
    /// Our column names.
    column_names: Vec<String>,
}

impl LibPostal {
    /// Create a new LibPostal geocoder.
    pub fn new() -> LibPostal {
        describe_counter!(
            "geocodecsv.addresses_parsed.total",
            "Total addresses parsed"
        );

        let column_names = COLUMN_NAMES
            .iter()
            .map(|&name| name.to_owned())
            .collect::<Vec<_>>();
        LibPostal { column_names }
    }

    pub async fn prime() {
        // "Prime" libpostal by forcing it to load its language model and associated data
        // into memory with a dummy call.
        let libpostal = LibPostal::new();
        let _ = libpostal
            .geocode_addresses(&[Address {
                street: "1 Main St".to_owned(),
                city: Some("Anytown".to_owned()),
                state: Some("VT".to_owned()),
                zipcode: None,
            }])
            .await;
    }
}

#[async_trait]
impl Geocoder for LibPostal {
    fn tag(&self) -> &str {
        "lp"
    }

    fn configuration_key(&self) -> &str {
        "default"
    }

    fn column_names(&self) -> &[String] {
        &self.column_names
    }

    async fn geocode_addresses(
        &self,
        addresses: &[Address],
    ) -> Result<Vec<Option<Geocoded>>> {
        let parse_opt = ParseAddressOptions::default();

        let mut result = Vec::with_capacity(addresses.len());
        for addr in addresses {
            // Turn our string into an address.
            let addr_str = format!(
                "{} {} {} {}",
                addr.street,
                addr.city_str(),
                addr.state_str(),
                addr.zipcode_str(),
            );

            // Parse it.
            let parsed = parse_address(&addr_str, &parse_opt)?;
            let mut geocoded = Geocoded {
                column_values: Vec::with_capacity(self.column_names.len()),
            };
            for &column_name in COLUMN_NAMES {
                if let Some(value) = parsed.get(column_name) {
                    geocoded.column_values.push(value.to_owned());
                } else {
                    geocoded.column_values.push("".to_owned());
                }
            }

            debug_assert_eq!(geocoded.column_values.len(), self.column_names().len());
            result.push(Some(geocoded));
        }
        counter!("geocodecsv.addresses_parsed.total", "parser" => "libpostal")
            .increment(result.len() as u64);
        Ok(result)
    }
}
