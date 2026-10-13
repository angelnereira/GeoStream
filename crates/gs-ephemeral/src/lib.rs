//! Ephemeral store trait and in-memory implementation (docs/11 §3, ADR-0010).
//!
//! The trait mirrors the keyspace of `docs/11_DATA_STORAGE.md` §3. Callers compose the keys
//! (`{p}:snap:{session}:{member}`, …) so that the memory and Redis/Valkey backends share the
//! same behaviour and the same test suite.
#![forbid(unsafe_code)]

use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use thiserror::Error;
use time::OffsetDateTime;

/// Result of an atomic sequence compare-and-set (docs/09 §4, ADR-0008).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SequenceCas {
    /// The write was applied; `previous` is the stored value before the write.
    Accepted { previous: u64 },
    /// The stored value already equals the written value (duplicate sample).
    Duplicate { current: u64 },
    /// The stored value is ahead of the written value (obsolete sample).
    Stale { current: u64 },
}

/// Errors returned by the ephemeral store.
#[derive(Debug, Error)]
pub enum EphemeralError {
    /// The backend is not reachable or lost the connection.
    #[error("ephemeral store unavailable: {0}")]
    Unavailable(String),

    /// The stored value does not have the expected shape.
    #[error("invalid value for ephemeral key '{key}': {message}")]
    InvalidValue { key: String, message: String },
}

/// Ephemeral, best-effort state: snapshots, sequences, presence, membership versions,
/// one-use tickets and rate limit counters (docs/11 §3).
#[allow(async_fn_in_trait)]
pub trait EphemeralStore: Send + Sync {
    /// Writes a snapshot with a TTL.
    async fn set_snapshot(
        &self,
        key: &str,
        value: &[u8],
        ttl: Duration,
    ) -> Result<(), EphemeralError>;

    /// Reads a snapshot, returning `None` when absent or expired.
    async fn get_snapshot(&self, key: &str) -> Result<Option<Vec<u8>>, EphemeralError>;

    /// Deletes a snapshot immediately (NFR-PRIV-002).
    async fn delete_snapshot(&self, key: &str) -> Result<bool, EphemeralError>;

    /// Atomic compare-and-set of the last accepted sequence.
    async fn compare_and_set_sequence(
        &self,
        key: &str,
        current: u64,
        next: u64,
    ) -> Result<SequenceCas, EphemeralError>;

    /// Sets the value only when it is greater than the stored one; returns the effective value.
    async fn set_if_greater(&self, key: &str, value: u64) -> Result<u64, EphemeralError>;

    /// Writes presence fields with a TTL (docs/04 §6.3).
    async fn set_presence(
        &self,
        key: &str,
        fields: &[(&str, &str)],
        ttl: Duration,
    ) -> Result<(), EphemeralError>;

    /// Reads presence fields, returning `None` when absent or expired.
    async fn get_presence(
        &self,
        key: &str,
    ) -> Result<Option<Vec<(String, String)>>, EphemeralError>;

    /// Deletes presence only when the stored `connection_id` matches (docs/11 §3).
    async fn release_presence(
        &self,
        key: &str,
        connection_id: &str,
    ) -> Result<bool, EphemeralError>;

    /// Reads the membership version of a session.
    async fn get_membership_version(&self, key: &str) -> Result<Option<u64>, EphemeralError>;

    /// Increments the membership version of a session and returns the new value.
    async fn increment_membership_version(&self, key: &str) -> Result<u64, EphemeralError>;

    /// Stores a stream ticket with a TTL (docs/06 §5).
    async fn set_ticket(
        &self,
        key: &str,
        grant: &[u8],
        ttl: Duration,
    ) -> Result<(), EphemeralError>;

    /// Atomically reads and deletes a ticket (single use, docs/06 §5).
    async fn take_ticket(&self, key: &str) -> Result<Option<Vec<u8>>, EphemeralError>;

    /// Increments a rate limit counter, resetting it when the window expires.
    async fn increment_counter(&self, key: &str, window: Duration) -> Result<u64, EphemeralError>;

    /// Reports whether the backend is usable (docs/04 §7.6).
    fn is_ready(&self) -> bool {
        true
    }
}

/// Stored value of the in-memory store.
#[derive(Debug, Clone)]
struct Entry {
    /// Raw bytes for snapshots, counter values as decimal text.
    bytes: Vec<u8>,
    /// Presence fields, when the entry is a hash.
    fields: Option<Vec<(String, String)>>,
    /// Expiry instant, `None` for keys without TTL.
    expires_at: Option<OffsetDateTime>,
}

impl Entry {
    fn is_expired(&self, now: OffsetDateTime) -> bool {
        self.expires_at.is_some_and(|expires_at| expires_at <= now)
    }

    fn touch(&mut self, now: OffsetDateTime) -> bool {
        if self.is_expired(now) {
            return false;
        }
        true
    }
}

/// In-memory ephemeral store: a single instance only (ADR-0010).
///
/// Entries expire lazily on access; `is_ready` reports the degradation state used by
/// `docs/04` §7.6. Nothing that is lost on restart is guaranteed.
#[derive(Debug, Default)]
pub struct InMemoryEphemeralStore {
    entries: DashMap<String, Entry>,
    ready: std::sync::atomic::AtomicBool,
}

impl InMemoryEphemeralStore {
    /// Creates a new, empty store.
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            entries: DashMap::new(),
            ready: std::sync::atomic::AtomicBool::new(true),
        })
    }

    /// Number of live entries: diagnostic helper for tests and metrics.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the store has no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn remove_if_expired(&self, key: &str, now: OffsetDateTime) {
        if self
            .entries
            .get(key)
            .is_some_and(|entry| entry.is_expired(now))
        {
            self.entries.remove(key);
        }
    }
}

impl EphemeralStore for InMemoryEphemeralStore {
    async fn set_snapshot(
        &self,
        key: &str,
        value: &[u8],
        ttl: Duration,
    ) -> Result<(), EphemeralError> {
        let now = OffsetDateTime::now_utc();
        self.remove_if_expired(key, now);
        self.entries.insert(
            key.to_string(),
            Entry {
                bytes: value.to_vec(),
                fields: None,
                expires_at: Some(now + ttl),
            },
        );
        Ok(())
    }

    async fn get_snapshot(&self, key: &str) -> Result<Option<Vec<u8>>, EphemeralError> {
        let now = OffsetDateTime::now_utc();
        self.remove_if_expired(key, now);
        Ok(self
            .entries
            .get(key)
            .filter(|entry| entry.fields.is_none())
            .map(|entry| entry.bytes.clone()))
    }

    async fn delete_snapshot(&self, key: &str) -> Result<bool, EphemeralError> {
        let now = OffsetDateTime::now_utc();
        self.remove_if_expired(key, now);
        Ok(self
            .entries
            .remove(key)
            .is_some_and(|(_, entry)| entry.fields.is_none()))
    }

    async fn compare_and_set_sequence(
        &self,
        key: &str,
        current: u64,
        next: u64,
    ) -> Result<SequenceCas, EphemeralError> {
        let now = OffsetDateTime::now_utc();
        self.remove_if_expired(key, now);

        // dashmap entry API holds the write lock for the whole comparison.
        let mut slot = self
            .entries
            .entry(key.to_string())
            .or_insert_with(|| Entry {
                bytes: current.to_string().into_bytes(),
                fields: None,
                expires_at: None,
            });
        if slot.fields.is_some() {
            return Err(EphemeralError::InvalidValue {
                key: key.to_string(),
                message: "sequence key stores presence fields".to_string(),
            });
        }

        let stored = parse_counter(slot.bytes.as_slice())?;
        if stored < next && current <= stored {
            slot.bytes = next.to_string().into_bytes();
            Ok(SequenceCas::Accepted { previous: stored })
        } else if stored == next {
            Ok(SequenceCas::Duplicate { current: stored })
        } else {
            Ok(SequenceCas::Stale { current: stored })
        }
    }

    async fn set_if_greater(&self, key: &str, value: u64) -> Result<u64, EphemeralError> {
        let now = OffsetDateTime::now_utc();
        self.remove_if_expired(key, now);

        let mut slot = self
            .entries
            .entry(key.to_string())
            .or_insert_with(|| Entry {
                bytes: 0u64.to_string().into_bytes(),
                fields: None,
                expires_at: None,
            });
        if slot.fields.is_some() {
            return Err(EphemeralError::InvalidValue {
                key: key.to_string(),
                message: "sequence key stores presence fields".to_string(),
            });
        }

        let stored = parse_counter(slot.bytes.as_slice())?;
        if value > stored {
            slot.bytes = value.to_string().into_bytes();
            Ok(value)
        } else {
            Ok(stored)
        }
    }

    async fn set_presence(
        &self,
        key: &str,
        fields: &[(&str, &str)],
        ttl: Duration,
    ) -> Result<(), EphemeralError> {
        let now = OffsetDateTime::now_utc();
        self.remove_if_expired(key, now);
        let owned: Vec<(String, String)> = fields
            .iter()
            .map(|(name, value)| ((*name).to_string(), (*value).to_string()))
            .collect();
        self.entries.insert(
            key.to_string(),
            Entry {
                bytes: Vec::new(),
                fields: Some(owned),
                expires_at: Some(now + ttl),
            },
        );
        Ok(())
    }

    async fn get_presence(
        &self,
        key: &str,
    ) -> Result<Option<Vec<(String, String)>>, EphemeralError> {
        let now = OffsetDateTime::now_utc();
        self.remove_if_expired(key, now);
        Ok(self.entries.get(key).and_then(|entry| entry.fields.clone()))
    }

    async fn release_presence(
        &self,
        key: &str,
        connection_id: &str,
    ) -> Result<bool, EphemeralError> {
        let now = OffsetDateTime::now_utc();
        self.remove_if_expired(key, now);
        let matches = self
            .entries
            .get(key)
            .and_then(|entry| entry.fields.clone())
            .and_then(|fields| {
                fields
                    .into_iter()
                    .find(|(name, _)| name == "connection_id")
                    .map(|(_, stored)| stored == connection_id)
            })
            .unwrap_or(false);
        if matches {
            self.entries.remove(key);
        }
        Ok(matches)
    }

    async fn get_membership_version(&self, key: &str) -> Result<Option<u64>, EphemeralError> {
        let now = OffsetDateTime::now_utc();
        self.remove_if_expired(key, now);
        match self.entries.get(key) {
            Some(entry) => Ok(Some(parse_counter(entry.bytes.as_slice())?)),
            None => Ok(None),
        }
    }

    async fn increment_membership_version(&self, key: &str) -> Result<u64, EphemeralError> {
        let now = OffsetDateTime::now_utc();
        self.remove_if_expired(key, now);

        let mut slot = self
            .entries
            .entry(key.to_string())
            .or_insert_with(|| Entry {
                bytes: 0u64.to_string().into_bytes(),
                fields: None,
                expires_at: None,
            });
        if slot.fields.is_some() {
            return Err(EphemeralError::InvalidValue {
                key: key.to_string(),
                message: "membership version key stores presence fields".to_string(),
            });
        }
        let next = parse_counter(slot.bytes.as_slice())? + 1;
        slot.bytes = next.to_string().into_bytes();
        Ok(next)
    }

    async fn set_ticket(
        &self,
        key: &str,
        grant: &[u8],
        ttl: Duration,
    ) -> Result<(), EphemeralError> {
        let now = OffsetDateTime::now_utc();
        self.remove_if_expired(key, now);
        self.entries.insert(
            key.to_string(),
            Entry {
                bytes: grant.to_vec(),
                fields: None,
                expires_at: Some(now + ttl),
            },
        );
        Ok(())
    }

    async fn take_ticket(&self, key: &str) -> Result<Option<Vec<u8>>, EphemeralError> {
        let now = OffsetDateTime::now_utc();
        self.remove_if_expired(key, now);
        Ok(self.entries.remove(key).and_then(|(_, entry)| {
            (entry.fields.is_none() && !entry.is_expired(now)).then(|| entry.bytes.clone())
        }))
    }

    async fn increment_counter(&self, key: &str, window: Duration) -> Result<u64, EphemeralError> {
        let now = OffsetDateTime::now_utc();
        self.remove_if_expired(key, now);

        let mut slot = self
            .entries
            .entry(key.to_string())
            .or_insert_with(|| Entry {
                bytes: 0u64.to_string().into_bytes(),
                fields: None,
                expires_at: Some(now + window),
            });
        if slot.fields.is_some() {
            return Err(EphemeralError::InvalidValue {
                key: key.to_string(),
                message: "counter key stores presence fields".to_string(),
            });
        }
        if !slot.touch(now) {
            slot.bytes = 0u64.to_string().into_bytes();
            slot.expires_at = Some(now + window);
        }
        let next = parse_counter(slot.bytes.as_slice())? + 1;
        slot.bytes = next.to_string().into_bytes();
        Ok(next)
    }

    fn is_ready(&self) -> bool {
        self.ready.load(std::sync::atomic::Ordering::SeqCst)
    }
}

/// Parses a decimal counter, rejecting anything else.
fn parse_counter(bytes: &[u8]) -> Result<u64, EphemeralError> {
    let text = std::str::from_utf8(bytes).map_err(|_| EphemeralError::InvalidValue {
        key: String::new(),
        message: "counter is not utf-8".to_string(),
    })?;
    text.parse::<u64>()
        .map_err(|e| EphemeralError::InvalidValue {
            key: String::new(),
            message: format!("counter is not a decimal integer: {e}"),
        })
}

impl<T: EphemeralStore + ?Sized> EphemeralStore for Arc<T> {
    async fn set_snapshot(
        &self,
        key: &str,
        value: &[u8],
        ttl: Duration,
    ) -> Result<(), EphemeralError> {
        (**self).set_snapshot(key, value, ttl).await
    }

    async fn get_snapshot(&self, key: &str) -> Result<Option<Vec<u8>>, EphemeralError> {
        (**self).get_snapshot(key).await
    }

    async fn delete_snapshot(&self, key: &str) -> Result<bool, EphemeralError> {
        (**self).delete_snapshot(key).await
    }

    async fn compare_and_set_sequence(
        &self,
        key: &str,
        current: u64,
        next: u64,
    ) -> Result<SequenceCas, EphemeralError> {
        (**self).compare_and_set_sequence(key, current, next).await
    }

    async fn set_if_greater(&self, key: &str, value: u64) -> Result<u64, EphemeralError> {
        (**self).set_if_greater(key, value).await
    }

    async fn set_presence(
        &self,
        key: &str,
        fields: &[(&str, &str)],
        ttl: Duration,
    ) -> Result<(), EphemeralError> {
        (**self).set_presence(key, fields, ttl).await
    }

    async fn get_presence(
        &self,
        key: &str,
    ) -> Result<Option<Vec<(String, String)>>, EphemeralError> {
        (**self).get_presence(key).await
    }

    async fn release_presence(
        &self,
        key: &str,
        connection_id: &str,
    ) -> Result<bool, EphemeralError> {
        (**self).release_presence(key, connection_id).await
    }

    async fn get_membership_version(&self, key: &str) -> Result<Option<u64>, EphemeralError> {
        (**self).get_membership_version(key).await
    }

    async fn increment_membership_version(&self, key: &str) -> Result<u64, EphemeralError> {
        (**self).increment_membership_version(key).await
    }

    async fn set_ticket(
        &self,
        key: &str,
        grant: &[u8],
        ttl: Duration,
    ) -> Result<(), EphemeralError> {
        (**self).set_ticket(key, grant, ttl).await
    }

    async fn take_ticket(&self, key: &str) -> Result<Option<Vec<u8>>, EphemeralError> {
        (**self).take_ticket(key).await
    }

    async fn increment_counter(&self, key: &str, window: Duration) -> Result<u64, EphemeralError> {
        (**self).increment_counter(key, window).await
    }

    fn is_ready(&self) -> bool {
        (**self).is_ready()
    }
}
