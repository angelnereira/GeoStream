//! Offline queue (`22` §3.5, ADR-0025).
//!
//! Pure, bounded, ordered. The **ciphertext** is what the queue holds: the
//! caller seals/opens with the platform `SecureStore` key before enqueuing and
//! after draining, so a dump of the queue never contains plaintext. This module
//! never sees a coordinate in the clear.
//!
//! Guarantees that the vectors `SMP-OFF-*` check:
//! - bounded: over `max_samples`, the **oldest** is dropped and the latest kept;
//! - ordered: batches preserve insertion order;
//! - age: samples older than `max_age_s` are dropped;
//! - purge: clearing is immediate and total (`NFR-PRIV-002`).

#![forbid(unsafe_code)]

use std::collections::VecDeque;

/// One sealed sample: an opaque ciphertext plus the timestamp used for ageing
/// and for `max_event_age_ms` decisions. `at_ms` is not secret.
#[derive(Debug, Clone)]
pub struct SealedSample {
    pub at_ms: u64,
    pub ciphertext: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct OfflineQueue {
    samples: VecDeque<SealedSample>,
    max_samples: usize,
    max_age_s: u64,
    /// Counters for diagnostics; never contain coordinates.
    pub dropped_oldest: u64,
    pub dropped_too_old: u64,
}

impl OfflineQueue {
    #[must_use]
    pub fn new(max_samples: usize, max_age_s: u64) -> Self {
        Self {
            samples: VecDeque::new(),
            max_samples,
            max_age_s,
            dropped_oldest: 0,
            dropped_too_old: 0,
        }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// Enqueue a sealed sample. If the queue is full, the **oldest** is dropped
    /// (`SMP-OFF-002`): the recent fix is what a subscriber needs.
    pub fn push(&mut self, sample: SealedSample) {
        if self.max_samples == 0 {
            self.dropped_oldest += 1;
            return;
        }
        while self.samples.len() >= self.max_samples {
            self.samples.pop_front();
            self.dropped_oldest += 1;
        }
        self.samples.push_back(sample);
    }

    /// Drop samples older than `max_age_s` relative to `now_ms` (`SMP-OFF-003`).
    pub fn expire(&mut self, now_ms: u64) {
        let max_age_ms = self.max_age_s.saturating_mul(1000);
        while let Some(front) = self.samples.front() {
            if now_ms.saturating_sub(front.at_ms) > max_age_ms {
                self.samples.pop_front();
                self.dropped_too_old += 1;
            } else {
                break;
            }
        }
    }

    /// Split the queue into batches of at most `batch_size`, preserving order
    /// (`SMP-OFF-001`). The queue is **not** modified: the caller removes the
    /// samples once the HTTP ingest confirms them, so a failed batch is retried.
    #[must_use]
    pub fn batches(&self, batch_size: usize) -> Vec<Vec<SealedSample>> {
        if batch_size == 0 {
            return Vec::new();
        }
        self.samples
            .iter()
            .cloned()
            .collect::<Vec<_>>()
            .chunks(batch_size)
            .map(<[SealedSample]>::to_vec)
            .collect()
    }

    /// Remove the first `n` samples (already delivered). Returns how many were
    /// actually removed.
    pub fn ack(&mut self, n: usize) -> usize {
        let removed = n.min(self.samples.len());
        for _ in 0..removed {
            self.samples.pop_front();
        }
        removed
    }

    /// Immediate, total purge (`SMP-OFF-004`, `NFR-PRIV-002`). Called when the
    /// subject stops sharing, leaves, is revoked or the session closes.
    pub fn purge(&mut self) {
        self.samples.clear();
        self.samples.shrink_to_fit();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sealed(at_ms: u64) -> SealedSample {
        SealedSample {
            at_ms,
            ciphertext: vec![0xAA; 8],
        }
    }

    #[test]
    fn smp_off_001_batches_preserve_order_and_size() {
        let mut q = OfflineQueue::new(20_000, 86_400);
        for i in 0..120u64 {
            q.push(sealed(i));
        }
        let batches = q.batches(50);
        assert_eq!(batches.len(), 3);
        assert!(batches.iter().all(|b| b.len() <= 50));
        // order preserved across the flattened result
        let flat: Vec<u64> = batches.iter().flatten().map(|s| s.at_ms).collect();
        let expected: Vec<u64> = (0..120).collect();
        assert_eq!(flat, expected);
    }

    #[test]
    fn smp_off_002_over_capacity_drops_oldest_and_keeps_latest() {
        let mut q = OfflineQueue::new(20_000, 86_400);
        for i in 0..20_050u64 {
            q.push(sealed(i));
        }
        assert_eq!(q.len(), 20_000);
        assert_eq!(q.dropped_oldest, 50);
        // latest retained
        let last = q.samples.back().unwrap();
        assert_eq!(last.at_ms, 20_049);
        let first = q.samples.front().unwrap();
        assert_eq!(first.at_ms, 50);
    }

    #[test]
    fn smp_off_003_expires_by_age() {
        let mut q = OfflineQueue::new(100, 10); // 10 s
        for i in 0..10u64 {
            q.push(sealed(i * 1000));
        }
        // now = 20 s: everything older than 10 s goes.
        q.expire(20_000);
        assert!(q.dropped_too_old > 0);
        assert!(q.samples.iter().all(|s| 20_000 - s.at_ms <= 10_000));
    }

    #[test]
    fn smp_off_004_purge_is_total_and_synchronous() {
        let mut q = OfflineQueue::new(20_000, 86_400);
        for i in 0..500u64 {
            q.push(sealed(i));
        }
        q.purge();
        assert_eq!(q.len(), 0);
        assert!(q.is_empty());
    }

    #[test]
    fn ack_removes_delivered_prefix() {
        let mut q = OfflineQueue::new(100, 100);
        for i in 0..10u64 {
            q.push(sealed(i));
        }
        assert_eq!(q.ack(4), 4);
        assert_eq!(q.len(), 6);
        assert_eq!(q.samples.front().unwrap().at_ms, 4);
        assert_eq!(q.ack(99), 6);
        assert!(q.is_empty());
    }
}
