//! Skip invalid addresses and don't pass them through to the next layer.

use async_trait::async_trait;
use metrics::{counter, describe_counter};

use crate::addresses::Address;

use super::{Geocoded, Geocoder, Result};

/// Skip invalid addresses and don't pass them through to the next layer.
pub struct InvalidRecordSkipper {
    // Our inner geocoder that we're normalizing for.
    inner: Box<dyn Geocoder>,
}

impl InvalidRecordSkipper {
    /// Create a new `Normalizer` wrapping the specified geocoder.
    pub fn new(inner: Box<dyn Geocoder>) -> InvalidRecordSkipper {
        describe_counter!(
            "geocodecsv.invalid_records.total",
            "Invalid records which could not be geocoded"
        );

        InvalidRecordSkipper { inner }
    }
}

#[async_trait]
impl Geocoder for InvalidRecordSkipper {
    fn tag(&self) -> &str {
        // We don't change anything which could possibly affect caching,
        // so we can just use our inner tag.
        self.inner.tag()
    }

    fn configuration_key(&self) -> &str {
        self.inner.configuration_key()
    }

    fn column_names(&self) -> &[String] {
        self.inner.column_names()
    }

    async fn geocode_addresses(
        &self,
        addresses: &[Address],
    ) -> Result<Vec<Option<Geocoded>>> {
        // Extract our valid addresses, keeping track of their original
        // positions.
        let mut original_indices = vec![];
        let mut valid_addresses = vec![];
        for (i, address) in addresses.iter().enumerate() {
            if address.is_valid() {
                valid_addresses.push(address.clone());
                original_indices.push(i);
            } else {
                counter!("geocodecsv.invalid_records.total", 1);
            }
        }

        // Geocode our valid addresses.
        let geocodeded = self.inner.geocode_addresses(&valid_addresses).await?;

        // Rebuild our geocoded addresses, inserting `None` for invalid.
        let mut result = vec![None; addresses.len()];
        for (i, geocoded) in geocodeded.into_iter().enumerate() {
            let original_index = original_indices[i];
            result[original_index] = geocoded;
        }
        Ok(result)
    }
}
