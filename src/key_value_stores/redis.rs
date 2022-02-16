//! A simple Redis client.

use std::time::Instant;

use anyhow::Context;
use async_trait::async_trait;
use bb8::{Pool, PooledConnection};
use bb8_redis::RedisConnectionManager;
use metrics::{describe_histogram, histogram, Unit};
use redis::{pipe, Pipeline};
use tracing::instrument;
use url::Url;

use crate::Result;

use super::{KeyValueStore, KeyValueStoreNew, PipelinedGet, PipelinedSet};

/// A simple Redis client.
pub struct Redis {
    /// Our Redis client pool.
    pool: Pool<RedisConnectionManager>,

    /// The prefix to use for our keys.
    key_prefix: String,
}

impl Redis {
    /// Get a Redis client.
    async fn client(&self) -> Result<PooledConnection<'_, RedisConnectionManager>> {
        self.pool.get().await.context("could not get Redis client")
    }
}

impl KeyValueStore for Redis {
    fn new_pipelined_get<'store>(
        &'store self,
    ) -> Box<dyn super::PipelinedGet<'store> + 'store> {
        Box::new(RedisPipelinedGet {
            redis: self,
            pipeline: pipe(),
        })
    }

    fn new_pipelined_set<'store>(
        &'store self,
    ) -> Box<dyn super::PipelinedSet<'store> + 'store> {
        Box::new(RedisPipelinedSet {
            redis: self,
            pipeline: pipe(),
        })
    }

    fn key_prefix(&self) -> &str {
        &self.key_prefix
    }
}

#[async_trait]
impl KeyValueStoreNew for Redis {
    #[instrument(name = "Redis::new", level = "trace", skip_all)]
    async fn new(url: Url, key_prefix: String) -> Result<Self> {
        describe_histogram!(
            "geocodecsv.redis.get_request.duration_seconds",
            Unit::Seconds,
            "Time required for Redis GET requests"
        );
        describe_histogram!(
            "geocodecsv.redis.set_request.duration_seconds",
            Unit::Seconds,
            "Time required for Redis SET requests"
        );

        let manager = RedisConnectionManager::new(url)
            .context("could not create Redis connection manager")?;
        let pool = Pool::builder()
            .build(manager)
            .await
            .context("could not create Redis connection pool")?;
        Ok(Redis { pool, key_prefix })
    }
}

/// A pipeline of GET operations.
struct RedisPipelinedGet<'store> {
    redis: &'store Redis,
    pipeline: Pipeline,
}

#[async_trait]
impl<'store> PipelinedGet<'store> for RedisPipelinedGet<'store> {
    fn add_get(&mut self, mut key: String) {
        self.redis.prefix_key(&mut key);
        self.pipeline.cmd("GET").arg(key);
    }

    #[instrument(name = "PipelinedGet::execute", level = "trace", skip_all)]
    async fn execute(&self) -> Result<Vec<Option<Vec<u8>>>> {
        let start = Instant::now();

        let mut client = self.redis.client().await?;
        let result = self
            .pipeline
            .query_async(&mut *client)
            .await
            .context("could not fetch keys from Redis")?;

        histogram!(
            "geocodecsv.redis.get_request.duration_seconds",
            (Instant::now() - start).as_secs_f64(),
        );

        Ok(result)
    }
}

/// A pipeline of SET operations.
struct RedisPipelinedSet<'store> {
    redis: &'store Redis,
    pipeline: Pipeline,
}

#[async_trait]
impl<'store> PipelinedSet<'store> for RedisPipelinedSet<'store> {
    fn add_set(&mut self, mut key: String, value: Vec<u8>) {
        self.redis.prefix_key(&mut key);
        self.pipeline.cmd("SET").arg(key).arg(value).ignore();
    }

    #[instrument(name = "PipelinedSet::execute", level = "trace", skip_all)]
    async fn execute(&self) -> Result<()> {
        let start = Instant::now();

        let mut client = self.redis.client().await?;
        let result = self
            .pipeline
            .query_async(&mut *client)
            .await
            .context("could not fetch keys from Redis")?;

        histogram!(
            "geocodecsv.redis.set_request.duration_seconds",
            (Instant::now() - start).as_secs_f64(),
        );

        Ok(result)
    }
}
