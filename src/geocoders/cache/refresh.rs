//! Policy for automatically refreshing stale cache entries.
//!
//! Each entry carries its own schedule (`refresh_attempts` plus its write age).
//! Failures back off exponentially and stop after `failure_max_attempts`.
//! Successes refresh only if `success_period` is set. A per-key-per-day rate
//! cap bounds spend so a bulk-loaded backlog does not all come due on day one.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::entry::CacheEntry;

/// Whether a cached entry recorded a successful geocode or a failure to match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CachedOutcome {
    Success,
    Failure,
}

impl CachedOutcome {
    /// A low-arity label for use with metrics.
    pub fn as_metric_label(self) -> &'static str {
        match self {
            CachedOutcome::Success => "success",
            CachedOutcome::Failure => "failure",
        }
    }
}

/// Named arguments for [`RefreshPolicy::new`].
pub struct RefreshPolicyConfig {
    /// Base wait before the first refresh of a cached failure.
    pub failure_period: Option<Duration>,

    /// Stop re-checking a failure after this many refreshes.
    pub failure_max_attempts: u32,

    /// If set, re-check a successful geocode after this age.
    pub success_period: Option<Duration>,

    /// Fraction of eligible keys to refresh on a given day, in `(0, 1]`.
    pub rate: f64,
}

/// Policy for automatically refreshing stale cache entries.
#[derive(Debug, Clone, Copy)]
pub struct RefreshPolicy {
    failure_period: Option<Duration>,
    failure_max_attempts: u32,
    success_period: Option<Duration>,
    rate: f64,
}

impl RefreshPolicy {
    /// Build a policy. `rate` must be in `(0, 1]`.
    pub fn new(config: RefreshPolicyConfig) -> RefreshPolicy {
        debug_assert!(
            config.rate > 0.0 && config.rate <= 1.0,
            "refresh rate must be in (0, 1], got {}",
            config.rate
        );
        RefreshPolicy {
            failure_period: config.failure_period,
            failure_max_attempts: config.failure_max_attempts,
            success_period: config.success_period,
            rate: config.rate,
        }
    }

    /// Should this cached `entry` be treated as a miss at wall-clock `now`?
    pub fn should_refresh(
        &self,
        cache_key: &str,
        entry: &CacheEntry,
        age: Duration,
        now: SystemTime,
    ) -> bool {
        if !self.is_eligible(entry, age) {
            return false;
        }
        let day_number = now
            .duration_since(UNIX_EPOCH)
            .expect("now is after the UNIX epoch")
            .as_secs()
            / (24 * 60 * 60);
        stable_fraction(&format!("{cache_key}:{day_number}")) < self.rate
    }

    fn is_eligible(&self, entry: &CacheEntry, age: Duration) -> bool {
        match entry.outcome() {
            CachedOutcome::Success => self
                .success_period
                .map(|period| age >= period)
                .unwrap_or(false),
            CachedOutcome::Failure => {
                let Some(period) = self.failure_period else {
                    return false;
                };
                if entry.refresh_attempts >= self.failure_max_attempts {
                    return false;
                }
                age >= backoff_wait(period, entry.refresh_attempts)
            }
        }
    }
}

/// Wait before the next refresh: `period * 2^refresh_attempts`.
fn backoff_wait(period: Duration, refresh_attempts: u32) -> Duration {
    period.saturating_mul(1u32 << refresh_attempts.min(31))
}

/// Map a string to a stable pseudo-random fraction in `[0.0, 1.0)`.
///
/// Deriving the decision from the key (and, at the call site, the day) means
/// the same entry makes the same decision on every read that day.
fn stable_fraction(material: &str) -> f64 {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(material.as_bytes());
    let leading =
        u64::from_be_bytes(digest[..8].try_into().expect("SHA-256 is 32 bytes"));
    leading as f64 / 2f64.powi(64)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: Duration = Duration::from_secs(24 * 60 * 60);

    fn now() -> SystemTime {
        UNIX_EPOCH + 20_000 * DAY
    }

    fn failure(refresh_attempts: u32) -> CacheEntry {
        CacheEntry {
            geocoded: None,
            refresh_attempts,
        }
    }

    fn success() -> CacheEntry {
        CacheEntry {
            geocoded: Some(vec!["ok".to_owned()]),
            refresh_attempts: 0,
        }
    }

    fn policy(config: RefreshPolicyConfig) -> RefreshPolicy {
        RefreshPolicy::new(config)
    }

    fn failures_only(rate: f64) -> RefreshPolicy {
        policy(RefreshPolicyConfig {
            failure_period: Some(90 * DAY),
            failure_max_attempts: 4,
            success_period: None,
            rate,
        })
    }

    #[test]
    fn failure_not_eligible_before_base_period() {
        let refresh_policy = failures_only(1.0);
        assert!(!refresh_policy.should_refresh("k", &failure(0), 89 * DAY, now(),));
        assert!(refresh_policy.should_refresh("k", &failure(0), 90 * DAY, now(),));
    }

    #[test]
    fn failure_backoff_doubles_each_attempt() {
        let refresh_policy = failures_only(1.0);
        assert!(!refresh_policy.should_refresh("k", &failure(1), 179 * DAY, now(),));
        assert!(refresh_policy.should_refresh("k", &failure(1), 180 * DAY, now(),));
        assert!(!refresh_policy.should_refresh("k", &failure(2), 359 * DAY, now(),));
        assert!(refresh_policy.should_refresh("k", &failure(2), 360 * DAY, now(),));
    }

    #[test]
    fn failure_stops_at_max_attempts() {
        let refresh_policy = failures_only(1.0);
        assert!(!refresh_policy.should_refresh("k", &failure(4), 10_000 * DAY, now(),));
    }

    #[test]
    fn failures_ignored_when_period_is_none() {
        let refresh_policy = policy(RefreshPolicyConfig {
            failure_period: None,
            failure_max_attempts: 4,
            success_period: Some(365 * DAY),
            rate: 1.0,
        });
        assert!(!refresh_policy.should_refresh("k", &failure(0), 10_000 * DAY, now(),));
    }

    #[test]
    fn successes_never_refresh_without_a_period() {
        let refresh_policy = failures_only(1.0);
        assert!(!refresh_policy.should_refresh("k", &success(), 10_000 * DAY, now(),));
    }

    #[test]
    fn successes_refresh_after_period() {
        let refresh_policy = policy(RefreshPolicyConfig {
            failure_period: None,
            failure_max_attempts: 0,
            success_period: Some(365 * DAY),
            rate: 1.0,
        });
        assert!(!refresh_policy.should_refresh("k", &success(), 364 * DAY, now(),));
        assert!(refresh_policy.should_refresh("k", &success(), 365 * DAY, now(),));
    }

    #[test]
    fn rate_one_refreshes_every_eligible_key() {
        let refresh_policy = failures_only(1.0);
        let released = (0..1000)
            .filter(|i| {
                refresh_policy.should_refresh(
                    &format!("key-{i}"),
                    &failure(0),
                    10_000 * DAY,
                    now(),
                )
            })
            .count();
        assert_eq!(released, 1000);
    }

    #[test]
    fn rate_cap_bounds_spend() {
        let refresh_policy = failures_only(0.1);
        let released = (0..1000)
            .filter(|i| {
                refresh_policy.should_refresh(
                    &format!("key-{i}"),
                    &failure(0),
                    10_000 * DAY,
                    now(),
                )
            })
            .count();
        assert!(
            (50..150).contains(&released),
            "expected ~100/1000 at rate 0.1, got {released}"
        );
    }

    #[test]
    fn decision_is_stable_for_a_key_on_the_same_day() {
        let refresh_policy = failures_only(0.5);
        let first = refresh_policy.should_refresh(
            "123 main st",
            &failure(0),
            10_000 * DAY,
            now(),
        );
        for _ in 0..100 {
            assert_eq!(
                refresh_policy.should_refresh(
                    "123 main st",
                    &failure(0),
                    10_000 * DAY,
                    now(),
                ),
                first,
            );
        }
    }

    #[test]
    fn decision_can_change_on_a_new_day() {
        let refresh_policy = failures_only(0.5);
        let mut saw_difference = false;
        for i in 0..200 {
            let cache_key = format!("key-{i}");
            let today = refresh_policy.should_refresh(
                &cache_key,
                &failure(0),
                10_000 * DAY,
                now(),
            );
            let tomorrow = refresh_policy.should_refresh(
                &cache_key,
                &failure(0),
                10_000 * DAY,
                now() + DAY,
            );
            if today != tomorrow {
                saw_difference = true;
                break;
            }
        }
        assert!(
            saw_difference,
            "hashing the day into the key should reshuffle some decisions"
        );
    }

    #[test]
    fn stable_fraction_is_in_range() {
        for i in 0..1000 {
            let fraction = stable_fraction(&format!("key-{i}:100"));
            assert!((0.0..1.0).contains(&fraction));
        }
    }
}
