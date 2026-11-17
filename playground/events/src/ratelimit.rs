//! Per-client token bucket, in memory only.
//!
//! The client address is used only as a map key, and not even directly: it
//! is hashed with a per-process random key (std's `RandomState`), so the
//! map holds 64-bit hashes, never addresses. Nothing here is logged or
//! written anywhere, and the state vanishes when the instance stops.

use std::collections::hash_map::RandomState;
use std::collections::HashMap;
use std::hash::BuildHasher;
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub struct RateLimiter {
    keyer: RandomState,
    buckets: Mutex<HashMap<u64, (f64, Instant)>>,
    burst: f64,
    per_second: f64,
}

/// Forget buckets idle this long (they'd be full again anyway).
const IDLE: Duration = Duration::from_secs(600);
const SWEEP_AT: usize = 10_000;

impl RateLimiter {
    pub fn new(burst: u32, per_second: f64) -> Self {
        RateLimiter {
            keyer: RandomState::new(),
            buckets: Mutex::new(HashMap::new()),
            burst: burst.max(1) as f64,
            per_second: per_second.max(0.001),
        }
    }

    /// Take one token for `ip`. `false` means over the limit.
    pub fn allow(&self, ip: IpAddr) -> bool {
        let key = self.keyer.hash_one(ip);
        let now = Instant::now();
        let mut map = self.buckets.lock().unwrap_or_else(|e| e.into_inner());
        if map.len() >= SWEEP_AT {
            map.retain(|_, (_, t)| now.duration_since(*t) < IDLE);
        }
        let (tokens, last) = map.entry(key).or_insert((self.burst, now));
        *tokens =
            (*tokens + now.duration_since(*last).as_secs_f64() * self.per_second).min(self.burst);
        *last = now;
        if *tokens >= 1.0 {
            *tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn burst_then_limited_per_client() {
        let rl = RateLimiter::new(3, 0.001);
        let a: IpAddr = "192.0.2.1".parse().unwrap();
        let b: IpAddr = "192.0.2.2".parse().unwrap();
        assert!(rl.allow(a) && rl.allow(a) && rl.allow(a));
        assert!(!rl.allow(a));
        assert!(rl.allow(b));
    }
}
