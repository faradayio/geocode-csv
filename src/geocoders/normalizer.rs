//! Normalize addresses using libpostal _before_ passing them to another
//! geocoder.

use std::collections::HashMap;

use async_trait::async_trait;
use metrics::{counter, describe_counter};
use tracing::trace;

use crate::addresses::Address;

use super::{libpostal::LibPostal, Geocoded, Geocoder, Result};

/// Normalize geocodes and pass them through to another geocoder.
pub struct Normalizer {
    // Our inner geocoder that we're normalizing for.
    inner: Box<dyn Geocoder>,

    // Our normalizer.
    libpostal: LibPostal,

    // A quick mapping from `libpostal` component names back to the column
    // indices in `libpostal`'s output.
    libpostal_component_indices: HashMap<String, usize>,
}

impl Normalizer {
    /// Create a new `Normalizer` wrapping the specified geocoder.
    pub fn new(inner: Box<dyn Geocoder>) -> Normalizer {
        describe_counter!(
            "geocodecsv.addresses_normalized.total",
            "Addresses changed by normalization"
        );

        let libpostal = LibPostal::new();
        let mut libpostal_component_indices = HashMap::new();
        for (i, column_name) in libpostal.column_names().iter().enumerate() {
            libpostal_component_indices.insert(column_name.to_owned(), i);
        }

        Normalizer {
            inner,
            libpostal,
            libpostal_component_indices,
        }
    }
}

#[async_trait]
impl Geocoder for Normalizer {
    fn tag(&self) -> &str {
        // TODO: We should probably incorporate our inner tag as well.
        "norm"
    }

    fn configuration_key(&self) -> &str {
        // TODO: Include libpostal configuration, too?
        self.inner.configuration_key()
    }

    fn column_names(&self) -> &[String] {
        self.inner.column_names()
    }

    async fn geocode_addresses(
        &self,
        addresses: &[Address],
    ) -> Result<Vec<Option<Geocoded>>> {
        // Geocode using libpostal first.
        let normalized = self.libpostal.geocode_addresses(addresses).await?;

        // Convert back to `Address` structs (more or less).
        let mut normalized_addresses = vec![];
        for (i, raw) in normalized.iter().enumerate() {
            match raw {
                None => {
                    // If our normalizer gave up completely (which I don't think
                    // ever happens?), pass through the original address.
                    normalized_addresses.push(addresses[i].clone());
                }
                Some(raw) => {
                    let normalized_address =
                        normalized_to_address(&self.libpostal_component_indices, raw);
                    if !normalized_address.eq_ignore_ascii_case(&addresses[i]) {
                        // Only count addresses that we've actually changed in
                        // some way.
                        counter!(
                            "geocodecsv.addresses_normalized.total",
                            1,
                            "normalizer" => "libpostal"
                        );
                        trace!(
                            "normalized {:?} to {:?}",
                            &addresses[i],
                            normalized_address
                        );
                    }
                    normalized_addresses.push(normalized_address);
                }
            }
        }

        // Pass normalized addresses to our inner geocoder.
        self.inner.geocode_addresses(&normalized_addresses).await
    }
}

/// Convert `normalized` back into an `Address`.
fn normalized_to_address(
    component_indices: &HashMap<String, usize>,
    normalized: &Geocoded,
) -> Address {
    // We preallocate our address fields with a some extra space before
    // they need to be reallocated.

    // Handle street addresses. This is very US centric and we should strongly
    // consider using https://crates.io/crates/address-formatter instead. Or at
    // least use it for non-US addresses.
    //
    // ☠️☠️☠️ CACHE WARNING ☠️☠️☠️: This needs to be _stable_, so we don't
    // invalidate our caches. And the addresses don't need to be legal postal
    // addresses. They just need to be good enough for our downstream geocoder
    // to figure out.
    let mut street = String::with_capacity(32);
    // PO boxes supposedly go before street addresses if both are present.
    append_component(component_indices, &mut street, normalized, "po_box");
    append_component(component_indices, &mut street, normalized, "house_number");
    // "house" is supposedly things like "Empire State Building".
    append_component(component_indices, &mut street, normalized, "house");
    append_component(component_indices, &mut street, normalized, "road");
    // "unit" is "appt A", or things like that. Lightly normalized.
    append_component(component_indices, &mut street, normalized, "unit");

    // Handle our city. "city_district" may be something like
    // "Brooklyn", with "city" either empty, or containing something
    // like "New York City".
    let mut city = String::with_capacity(32);
    append_component(component_indices, &mut city, normalized, "city_district");
    append_component(component_indices, &mut city, normalized, "city");

    // Handle our state.
    let mut state = String::with_capacity(16);
    append_component(component_indices, &mut state, normalized, "state");

    // Handle our zipcode.
    let mut zipcode = String::with_capacity(16);
    append_component(component_indices, &mut zipcode, normalized, "postcode");

    // Build our `Address`.
    Address {
        street,
        city: if city.is_empty() { None } else { Some(city) },
        state: if state.is_empty() { None } else { Some(state) },
        zipcode: if zipcode.is_empty() {
            None
        } else {
            Some(zipcode)
        },
    }
}

/// Append the component `component` from `normalized` to `base` (if we have it).
fn append_component(
    component_indices: &HashMap<String, usize>,
    base: &mut String,
    normalized: &Geocoded,
    component: &str,
) {
    let idx = component_indices
        .get(component)
        .expect("should only ever call this with valid address component names");
    if let Some(value) = normalized.column_values.get(*idx) {
        if !value.is_empty() {
            append_space_separated(base, value);
        }
    }
}

/// Append `to_append` to `base`, adding a leading space if we need one to
/// separate from an existing value in `base`.
fn append_space_separated(base: &mut String, to_append: &str) {
    if !base.is_empty() && !base.ends_with(' ') {
        base.push(' ');
    }
    // We once head a rumor of trailing spaces in `libpostal` output. This was
    // actually caused by an invalid `-spec.json` file, but it's cheap enough to
    // protect against this if it ever happened.
    base.push_str(to_append.trim());
}
