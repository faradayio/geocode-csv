//! On-disk format for a cached geocoding result.
//!
//! The first byte is a format tag. We always write `N`, the legacy tag, so
//! older binaries can still read the `Option<Vec<String>>` payload. Refresh
//! state is appended after that payload; old `decode_from_slice` callers ignore
//! the leftover bytes.
//!
//! - `N` plus only the option payload: a pre-refresh row (`refresh_attempts = 0`).
//! - `N` plus the option payload plus metadata: a current row.

use anyhow::{format_err, Context};
use metrics::{counter, describe_counter, Unit};
use serde::{Deserialize, Serialize};

use crate::Result;

use super::refresh::CachedOutcome;

/// A cached geocoding result, plus how many times we have re-checked a failure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheEntry {
    /// Column values from a successful geocode, or `None` for a cached miss.
    pub geocoded: Option<Vec<String>>,

    /// How many times we have refreshed this entry and still got a failure.
    /// Reset to 0 on a successful geocode.
    pub refresh_attempts: u32,
}

const FORMAT_LEGACY: u8 = b'N';
const METADATA_VERSION: u8 = 1;

fn bincode_config() -> impl bincode::config::Config {
    bincode::config::standard()
        .with_little_endian()
        .with_variable_int_encoding()
}

fn refresh_attempts_from_trailing_bytes(trailing: &[u8]) -> Result<u32> {
    match trailing {
        [] => Ok(0),
        [METADATA_VERSION, rest @ ..] if rest.len() >= 4 => Ok(u32::from_le_bytes(
            rest[..4].try_into().expect("slice is 4 bytes"),
        )),
        [METADATA_VERSION, ..] => Err(format_err!("truncated cache refresh metadata")),
        [other, ..] => Err(format_err!(
            "unknown cache refresh metadata version {other}"
        )),
    }
}

impl CacheEntry {
    /// Register the cache payload size counters (same names as the old
    /// compressor, so existing dashboards keep working).
    pub fn describe_metrics() {
        describe_counter!(
            "geocodecsv.compressor_input.bytes_total",
            Unit::Bytes,
            "Bytes input to compressor"
        );
        describe_counter!(
            "geocodecsv.compressor_output.bytes_total",
            Unit::Bytes,
            "Bytes output by compressor"
        );
        describe_counter!(
            "geocodecsv.decompressor_input.bytes_total",
            Unit::Bytes,
            "Bytes input to decompressor"
        );
        describe_counter!(
            "geocodecsv.decompressor_output.bytes_total",
            Unit::Bytes,
            "Bytes output by decompressor"
        );
    }

    /// Build the entry we write after a geocode.
    ///
    /// A refreshed failure increments the prior attempt count. A success, or a
    /// first-time miss, writes 0. If we refresh a success and the geocoder
    /// returns no match, keep the prior success instead of replacing it.
    pub fn for_write_back(
        geocoded: Option<Vec<String>>,
        prior_entry: Option<&CacheEntry>,
    ) -> CacheEntry {
        match (geocoded, prior_entry) {
            (None, Some(prior_entry)) if prior_entry.geocoded.is_some() => {
                CacheEntry {
                    geocoded: prior_entry.geocoded.clone(),
                    refresh_attempts: 0,
                }
            }
            (None, Some(prior_entry)) => CacheEntry {
                geocoded: None,
                refresh_attempts: prior_entry.refresh_attempts.saturating_add(1),
            },
            (geocoded, _) => CacheEntry {
                geocoded,
                refresh_attempts: 0,
            },
        }
    }

    /// Whether this entry recorded a successful geocode or a failure to match.
    pub fn outcome(&self) -> CachedOutcome {
        if self.geocoded.is_some() {
            CachedOutcome::Success
        } else {
            CachedOutcome::Failure
        }
    }

    /// Encode this entry as an `N`-tagged payload that older binaries can read.
    pub fn encode(&self, output: &mut Vec<u8>) -> Result<()> {
        let mut payload = Vec::with_capacity(256);
        bincode::serde::encode_into_std_write(
            &self.geocoded,
            &mut payload,
            bincode_config(),
        )
        .context("could not encode value for caching")?;
        payload.push(METADATA_VERSION);
        payload.extend_from_slice(&self.refresh_attempts.to_le_bytes());
        counter!("geocodecsv.compressor_input.bytes_total", "compressor" => "none")
            .increment(payload.len() as u64);
        output.push(FORMAT_LEGACY);
        output.extend_from_slice(&payload);
        counter!("geocodecsv.compressor_output.bytes_total", "compressor" => "none")
            .increment((1 + payload.len()) as u64);
        Ok(())
    }

    /// Decode a tagged cache payload.
    pub fn decode(bytes: &[u8]) -> Result<CacheEntry> {
        let Some((format_tag, payload)) = bytes.split_first() else {
            return Err(format_err!("cached value is empty"));
        };
        counter!("geocodecsv.decompressor_input.bytes_total", "compressor" => "none")
            .increment(payload.len() as u64);
        let entry = match *format_tag {
            FORMAT_LEGACY => {
                let (geocoded, consumed) = bincode::serde::decode_from_slice::<
                    Option<Vec<String>>,
                    _,
                >(payload, bincode_config())
                .context("could not deserialize cached data")?;
                CacheEntry {
                    geocoded,
                    refresh_attempts: refresh_attempts_from_trailing_bytes(
                        &payload[consumed..],
                    )?,
                }
            }
            other => {
                return Err(format_err!("unknown cache format {:?}", other));
            }
        };
        counter!("geocodecsv.decompressor_output.bytes_total", "compressor" => "none")
            .increment(payload.len() as u64);
        Ok(entry)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode_legacy(geocoded: Option<Vec<String>>) -> Vec<u8> {
        let mut payload = Vec::new();
        bincode::serde::encode_into_std_write(
            &geocoded,
            &mut payload,
            bincode_config(),
        )
        .unwrap();
        let mut bytes = vec![FORMAT_LEGACY];
        bytes.extend_from_slice(&payload);
        bytes
    }

    #[test]
    fn round_trip_entry() {
        let original = CacheEntry {
            geocoded: Some(vec!["a".to_owned(), "b".to_owned()]),
            refresh_attempts: 3,
        };
        let mut encoded = Vec::new();
        original.encode(&mut encoded).unwrap();
        assert_eq!(encoded[0], FORMAT_LEGACY);
        assert_eq!(CacheEntry::decode(&encoded).unwrap(), original);
    }

    #[test]
    fn round_trip_failure() {
        let original = CacheEntry {
            geocoded: None,
            refresh_attempts: 2,
        };
        let mut encoded = Vec::new();
        original.encode(&mut encoded).unwrap();
        assert_eq!(CacheEntry::decode(&encoded).unwrap(), original);
    }

    #[test]
    fn new_rows_are_readable_by_the_legacy_option_decoder() {
        let original = CacheEntry {
            geocoded: Some(vec!["x".to_owned()]),
            refresh_attempts: 4,
        };
        let mut encoded = Vec::new();
        original.encode(&mut encoded).unwrap();
        let (geocoded, _consumed) = bincode::serde::decode_from_slice::<
            Option<Vec<String>>,
            _,
        >(&encoded[1..], bincode_config())
        .unwrap();
        assert_eq!(geocoded, Some(vec!["x".to_owned()]));
    }

    #[test]
    fn legacy_success_decodes_with_zero_attempts() {
        let bytes = encode_legacy(Some(vec!["x".to_owned()]));
        assert_eq!(
            CacheEntry::decode(&bytes).unwrap(),
            CacheEntry {
                geocoded: Some(vec!["x".to_owned()]),
                refresh_attempts: 0,
            }
        );
    }

    #[test]
    fn legacy_failure_decodes_with_zero_attempts() {
        let bytes = encode_legacy(None);
        assert_eq!(
            CacheEntry::decode(&bytes).unwrap(),
            CacheEntry {
                geocoded: None,
                refresh_attempts: 0,
            }
        );
    }

    #[test]
    fn write_back_increments_failure_attempts() {
        let prior = CacheEntry {
            geocoded: None,
            refresh_attempts: 2,
        };
        assert_eq!(
            CacheEntry::for_write_back(None, Some(&prior)).refresh_attempts,
            3
        );
        assert_eq!(
            CacheEntry::for_write_back(Some(vec!["ok".to_owned()]), Some(&prior))
                .refresh_attempts,
            0
        );
        assert_eq!(CacheEntry::for_write_back(None, None).refresh_attempts, 0);
    }

    #[test]
    fn write_back_keeps_a_success_if_refresh_fails() {
        let prior = CacheEntry {
            geocoded: Some(vec!["kept".to_owned()]),
            refresh_attempts: 0,
        };
        assert_eq!(
            CacheEntry::for_write_back(None, Some(&prior)),
            CacheEntry {
                geocoded: Some(vec!["kept".to_owned()]),
                refresh_attempts: 0,
            }
        );
    }

    #[test]
    fn empty_payload_is_an_error() {
        assert!(CacheEntry::decode(&[]).is_err());
    }

    #[test]
    fn unknown_format_is_an_error() {
        assert!(CacheEntry::decode(&[b'Z', 1, 2, 3]).is_err());
    }
}
