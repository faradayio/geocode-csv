//! Redis-based caching layer (because Redis is one of the few things fast
//! enough to handle a cluster of geocode-csv clients running at full speed).

use std::fmt::{self, Write};

use anyhow::{format_err, Context};
use async_trait::async_trait;
use metrics::{counter, describe_counter};

use crate::{addresses::Address, key_value_stores::KeyValueStore, Result};

use self::compression::CacheCompressor;

use super::{Geocoded, Geocoder};

mod compression;

/// A Redis-based caching layer.
///
/// This wraps another geocoder, and caches calls in Redis.
pub struct Cache {
    /// Compressor we use to compress cached data.
    compressor: CacheCompressor,

    /// Our key/value store.
    key_value_store: Box<dyn KeyValueStore>,

    /// The geocoder we're wrapping.
    inner: Box<dyn Geocoder>,

    /// The cache key for `inner`.
    inner_cache_prefix: String,

    /// Should we record our cache keys in our output?
    output_keys: bool,

    /// Should we geocode cache misses?
    cache_hits_only: bool,

    /// The column names we output.
    column_names: Vec<String>,
}

impl Cache {
    /// Create a new cache wrapping `inner`, and storing values in
    /// `key_value_store`.
    pub async fn new(
        key_value_store: Box<dyn KeyValueStore>,
        inner: Box<dyn Geocoder>,
        output_keys: bool,
        cache_hits_only: bool,
    ) -> Result<Cache> {
        describe_counter!("geocodecsv.cache_hits.total", "Addresses found in cache");
        describe_counter!(
            "geocodecsv.cache_misses.total",
            "Addresses not found in cache"
        );

        let inner_cache_prefix = inner.cache_prefix();
        let mut column_names = inner.column_names().to_owned();
        if output_keys {
            column_names.push("cache_key".to_owned());
        }

        Ok(Cache {
            compressor: CacheCompressor::new(),
            key_value_store,
            inner,
            inner_cache_prefix,
            output_keys,
            column_names,
            cache_hits_only,
        })
    }
}

#[async_trait]
impl Geocoder for Cache {
    fn tag(&self) -> &str {
        // TODO: We should probably incorporate our inner tag as well.
        "cache"
    }

    fn configuration_key(&self) -> &str {
        self.inner.configuration_key()
    }

    fn column_names(&self) -> &[String] {
        &self.column_names
    }

    async fn geocode_addresses(
        &self,
        addresses: &[Address],
    ) -> Result<Vec<Option<Geocoded>>> {
        // Build our list of keys.
        let keys = addresses
            .iter()
            .map(|addr| cache_key(&self.inner_cache_prefix, addr))
            .collect::<Vec<_>>();
        // Start with each geocoded address set to `None`.
        let mut geocoded = vec![None; addresses.len()];

        // If we have no records, don't call into the cache, because this may
        // cause weird problems, including hanging or running out of memory.
        // I suspect that there's an issue in the `bigtable` driver, but this
        // is a resonable thing to do anyway.
        if keys.is_empty() {
            return Ok(geocoded);
        }

        // TODO: De-duplicate duplicate addresses _within_ `addresses`.

        // Check to see what keys are stored in Redis.
        let mut pipelined_get = self.key_value_store.new_pipelined_get();
        for key in &keys {
            pipelined_get.add_get(key.to_owned());
        }
        let cache_results: Vec<Option<Vec<u8>>> = pipelined_get.execute().await?;

        // Our standard bincode configuration.
        let bincode_config = bincode::config::standard()
            .with_little_endian()
            .with_variable_int_encoding();

        // Unpack our results, recording any cache hits, and building a list of
        // the misses to forward to our inner geocoder.
        let mut cache_misses = Vec::with_capacity(addresses.len());
        let mut cache_miss_offsets = Vec::with_capacity(addresses.len());
        let mut decompressed = Vec::with_capacity(256);
        for (i, cached_value) in cache_results.iter().enumerate() {
            if let Some(cache_hit) = cached_value {
                // We found this result in the cache.
                decompressed.clear();
                if cache_hit[0] != self.compressor.id() {
                    return Err(format_err!(
                        "unknown compression format {:?}",
                        cache_hit[0]
                    ));
                }
                self.compressor
                    .decompress(&cache_hit[1..], &mut decompressed)?;
                let (cache_hit, _) = bincode::serde::decode_from_slice::<
                    Option<Vec<String>>,
                    _,
                >(&decompressed, bincode_config)
                .context("could not deserialize cached data")?;

                // Here, a `None` value represents a cached geocoding _failure_.
                // If a previous attempt failed, we expect that more recent ones
                // may fail, too.
                //
                // TODO: Explain this better.
                if let Some(cache_hit) = cache_hit {
                    if cache_hit.len() != self.inner.column_names().len() {
                        return Err(format_err!(
                            "cannot return {:?} for columns {:?} because it has the wrong number of values",
                            cache_hit,
                            self.column_names(),
                        ));
                    }
                    let candidate = Geocoded {
                        column_values: cache_hit,
                    };
                    if candidate.contains_null_bytes() {
                        // This result contains old buggy data. Treat as a cache
                        // miss.
                        cache_misses.push(addresses[i].clone());
                        cache_miss_offsets.push(i);
                        counter!(
                            "geocodecsv.cache_hits.total",
                            1,
                            "geocoding_result" => "invalid_data"
                        );
                    } else {
                        geocoded[i] = Some(candidate);
                        counter!(
                            "geocodecsv.cache_hits.total",
                            1,
                            "geocoding_result" => "found"
                        );
                    }
                } else {
                    counter!(
                        "geocodecsv.cache_hits.total",
                        1,
                        "geocoding_result" => "unknown_address"
                    );
                }
            } else {
                // We need to forward this result.
                cache_misses.push(addresses[i].clone());
                cache_miss_offsets.push(i);
            }
        }
        counter!("geocodecsv.cache_misses.total", cache_misses.len() as u64);
        drop(cache_results);

        // If we have any cache misses, deal with them.
        // Alternatively, if the caller specified --cache-hits-only,
        // we should avoid geocoding any remaining addresses.
        if !cache_misses.is_empty() && !self.cache_hits_only {
            // Pass remainder through to our inner geocoder.
            let cache_miss_retries =
                self.inner.geocode_addresses(&cache_misses).await?;

            // Record our successes (and build a Redis command to store them).
            let mut pipelined_set = self.key_value_store.new_pipelined_set();
            let mut encoded = Vec::with_capacity(256);
            for (i, retry) in cache_miss_offsets
                .into_iter()
                .zip(cache_miss_retries.into_iter())
            {
                // Encode our value for caching.
                let value = retry.as_ref().map(|retry| &retry.column_values);
                encoded.clear();
                bincode::encode_into_std_write(value, &mut encoded, bincode_config)
                    .context("could not encode value for caching")?;

                // Compress our encoded value and add it to our pipeline set.
                let mut compressed = Vec::with_capacity(256);
                compressed.push(self.compressor.id());
                self.compressor.compress(&encoded, &mut compressed)?;
                pipelined_set.add_set(keys[i].clone(), compressed);

                // Add out geocoding result to our output.
                geocoded[i] = retry;
            }

            // Write our new results back to our cache.
            pipelined_set.execute().await?;
        }

        // Output our cache key, too, if we were asked to do so.
        if self.output_keys {
            debug_assert_eq!(geocoded.len(), keys.len());
            for (result, key) in geocoded.iter_mut().zip(keys.iter()) {
                if let Some(result) = result {
                    result.column_values.push(key.to_owned());
                }
            }
        }

        Ok(geocoded)
    }
}

/// Given an address, build our cache key.
///
/// We convert this to lowercase to provide a _tiny_ level of normalization,
/// which may also help normalized mode (which always uses lowercase) and
/// unnormalized mode (which uses mixed case) to share more cache hits.
fn cache_key(cache_prefix: &str, addr: &Address) -> String {
    format!(
        "gcsv:{}:{}:{}:{}:{}",
        cache_prefix,
        EscapeColons(addr.state_str()),
        EscapeColons(addr.city_str()),
        EscapeColons(addr.zipcode_str()),
        EscapeColons(&addr.street),
    )
    .to_ascii_lowercase()
}

/// Escape colons in a string.
struct EscapeColons<'a>(&'a str);

impl<'a> fmt::Display for EscapeColons<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // This check is inefficient. We could do better.
        if self.0.contains('\\') || self.0.contains(':') {
            for c in self.0.chars() {
                if c == '\\' || c == ':' {
                    f.write_char('\\')?;
                }
                f.write_char(c)?;
            }
            Ok(())
        } else {
            self.0.fmt(f)
        }
    }
}

#[test]
fn escape_colons() {
    let examples = &[
        ("", ""),
        ("a", "a"),
        (":", "\\:"),
        ("\\", "\\\\"),
        ("abc\\def:ghi", "abc\\\\def\\:ghi"),
    ];
    for (input, expected) in examples {
        assert_eq!(format!("{}", EscapeColons(input)), *expected);
    }
}
