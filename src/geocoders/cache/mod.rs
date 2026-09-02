//! Redis-based caching layer (because Redis is one of the few things fast
//! enough to handle a cluster of geocode-csv clients running at full speed).

use std::fmt::{self, Write};
use std::time::SystemTime;

use anyhow::format_err;
use async_trait::async_trait;
use metrics::{counter, describe_counter};

use crate::{
    addresses::Address,
    key_value_stores::{CachedValue, KeyValueStore},
    Result,
};

use self::entry::CacheEntry;
use self::refresh::RefreshPolicy;

use super::{Geocoded, Geocoder};

mod entry;
pub mod refresh;

/// A Redis-based caching layer.
///
/// This wraps another geocoder, and caches calls in Redis.
pub struct Cache {
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

    /// Optional policy for automatically refreshing stale cache entries.
    refresh_policy: Option<RefreshPolicy>,

    /// The column names we output.
    column_names: Vec<String>,
}

/// An address we need to send to the inner geocoder.
struct CacheMiss {
    offset: usize,
    address: Address,
    prior_entry: Option<CacheEntry>,
}

impl Cache {
    /// Create a new cache wrapping `inner`, and storing values in
    /// `key_value_store`.
    pub async fn new(
        key_value_store: Box<dyn KeyValueStore>,
        inner: Box<dyn Geocoder>,
        output_keys: bool,
        cache_hits_only: bool,
        refresh_policy: Option<RefreshPolicy>,
    ) -> Result<Cache> {
        describe_counter!("geocodecsv.cache_hits.total", "Addresses found in cache");
        describe_counter!(
            "geocodecsv.cache_misses.total",
            "Addresses not found in cache"
        );
        describe_counter!(
            "geocodecsv.cache.refreshed.total",
            "Cache entries refreshed (treated as a miss) by the refresh policy"
        );
        describe_counter!(
            "geocodecsv.cache.refresh_results.total",
            "Outcomes of a refresh, labeled by the prior and new outcome"
        );
        CacheEntry::describe_metrics();

        let inner_cache_prefix = inner.cache_prefix();
        let mut column_names = inner.column_names().to_owned();
        if output_keys {
            column_names.push("cache_key".to_owned());
        }

        Ok(Cache {
            key_value_store,
            inner,
            inner_cache_prefix,
            output_keys,
            column_names,
            cache_hits_only,
            refresh_policy,
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
        let cache_results: Vec<Option<CachedValue>> = pipelined_get.execute().await?;

        // Unpack our results, recording any cache hits, and building a list of
        // the misses to forward to our inner geocoder.
        let mut cache_misses = Vec::with_capacity(addresses.len());
        // Compute the current time once for the whole batch, so every entry's
        // refresh decision is made against a single consistent `now`.
        let now = SystemTime::now();
        for (i, cached_value) in cache_results.iter().enumerate() {
            let Some(cached_value) = cached_value else {
                cache_misses.push(CacheMiss {
                    offset: i,
                    address: addresses[i].clone(),
                    prior_entry: None,
                });
                continue;
            };

            let entry = CacheEntry::decode(&cached_value.bytes)?;

            if let (Some(refresh_policy), Some(age), false) =
                (&self.refresh_policy, cached_value.age, self.cache_hits_only)
            {
                if refresh_policy.should_refresh(&keys[i], &entry, age, now) {
                    counter!("geocodecsv.cache.refreshed.total", "outcome" => entry.outcome().as_metric_label())
                        .increment(1);
                    cache_misses.push(CacheMiss {
                        offset: i,
                        address: addresses[i].clone(),
                        prior_entry: Some(entry),
                    });
                    continue;
                }
            }

            if let Some(column_values) = entry.geocoded {
                if column_values.len() != self.inner.column_names().len() {
                    return Err(format_err!(
                        "cannot return {:?} for columns {:?} because it has the wrong number of values",
                        column_values,
                        self.column_names(),
                    ));
                }
                let candidate = Geocoded { column_values };
                if candidate.contains_null_bytes() {
                    cache_misses.push(CacheMiss {
                        offset: i,
                        address: addresses[i].clone(),
                        prior_entry: None,
                    });
                    counter!("geocodecsv.cache_hits.total", "geocoding_result" => "invalid_data")
                        .increment(1);
                } else {
                    geocoded[i] = Some(candidate);
                    counter!("geocodecsv.cache_hits.total", "geocoding_result" => "found")
                        .increment(1);
                }
            } else {
                counter!("geocodecsv.cache_hits.total", "geocoding_result" => "unknown_address")
                    .increment(1);
            }
        }
        counter!("geocodecsv.cache_misses.total").increment(cache_misses.len() as u64);
        drop(cache_results);

        // If we have any cache misses, deal with them.
        // Alternatively, if the caller specified --cache-hits-only,
        // we should avoid geocoding any remaining addresses.
        if !cache_misses.is_empty() && !self.cache_hits_only {
            let cache_miss_addresses = cache_misses
                .iter()
                .map(|cache_miss| cache_miss.address.clone())
                .collect::<Vec<_>>();
            let cache_miss_retries =
                self.inner.geocode_addresses(&cache_miss_addresses).await?;

            let mut pipelined_set = self.key_value_store.new_pipelined_set();
            let mut encoded = Vec::with_capacity(256);
            for (cache_miss, retry) in cache_misses.into_iter().zip(cache_miss_retries)
            {
                if let Some(prior_entry) = &cache_miss.prior_entry {
                    counter!(
                        "geocodecsv.cache.refresh_results.total",
                        "from" => prior_entry.outcome().as_metric_label(),
                        "to" => if retry.is_some() { "success" } else { "failure" }
                    )
                    .increment(1);
                }

                let entry = CacheEntry::for_write_back(
                    retry.as_ref().map(|retry| retry.column_values.clone()),
                    cache_miss.prior_entry.as_ref(),
                );
                encoded.clear();
                entry.encode(&mut encoded)?;
                pipelined_set.add_set(
                    keys[cache_miss.offset].clone(),
                    std::mem::take(&mut encoded),
                );

                geocoded[cache_miss.offset] = entry
                    .geocoded
                    .map(|column_values| Geocoded { column_values });
            }

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
