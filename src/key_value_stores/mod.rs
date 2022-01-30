//! Common interface to key/value stores used for caching.

use anyhow::format_err;
use async_trait::async_trait;
use url::Url;

use crate::Result;

mod bigtable;
mod redis;

/// A key/value store, like Redis or BigTable.
///
/// We focus only on "pipelined" operations, where many requests are sent at
/// once, to avoid minimize network round trips.
pub trait KeyValueStore: Send + Sync + 'static {
    /// Create a new "pipelined" get request.
    fn new_pipelined_get<'store>(
        &'store self,
    ) -> Box<dyn PipelinedGet<'store> + 'store>;

    /// Create a new "pipelined" set request.
    fn new_pipelined_set<'store>(
        &'store self,
    ) -> Box<dyn PipelinedSet<'store> + 'store>;

    /// Get a prefix to use for all our keys. This should be the `key_prefix`
    /// parameter passed to `KeyValueStore::new_from_url`.
    fn key_prefix(&self) -> &str;

    /// Preprend `key_prefix` to `key`.
    fn prefix_key(&self, key: &mut String) {
        key.insert_str(0, self.key_prefix());
    }
}

impl dyn KeyValueStore {
    /// Create an appropriate `KeyValueStore` instance based on `url`.
    pub async fn new_from_url(
        url: Url,
        key_prefix: String,
    ) -> Result<Box<dyn KeyValueStore>> {
        match url.scheme() {
            "redis" => Ok(Box::new(redis::Redis::new(url, key_prefix).await?)),
            "bigtable" => {
                Ok(Box::new(bigtable::BigTable::new(url, key_prefix).await?))
            }
            scheme => {
                Err(format_err!("don't know how to connect to {}: URLs", scheme))
            }
        }
    }
}

/// An interface for creating a `KeyValueStore`.
///
/// This can't be part of `KeyValueStore` because we can't have static methods
/// like `new` on types that we'll put in a `Box`. (See [object-safe
/// traits](https://doc.rust-lang.org/book/ch17-02-trait-objects.html)).
#[async_trait]
pub trait KeyValueStoreNew: KeyValueStore + Sized {
    /// Create a new key/value store client.
    async fn new(url: Url, key_prefix: String) -> Result<Self>;
}

/// A series of "get" requests that we'll send in a single batch.
#[async_trait]
pub trait PipelinedGet<'store>: Send + Sync {
    /// Add a "get" request to our pipeline.
    fn add_get(&mut self, key: String);

    /// Execute all our requests and return the results in order. We return
    /// `None` when a value can't be found.
    async fn execute(&self) -> Result<Vec<Option<Vec<u8>>>>;
}

/// A series of "set" requests that we'll send in a single batch.
#[async_trait]
pub trait PipelinedSet<'store>: Send + Sync {
    /// Add a "set" request to our pipeline.
    fn add_set(&mut self, key: String, value: Vec<u8>);

    /// Execute all our requests.
    async fn execute(&self) -> Result<()>;
}
