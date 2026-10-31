//! Redis/Valkey implementation of [`EphemeralStore`] (docs/11 §3, ADR-0010, T-F6-01).
//!
//! Every multi-step operation is atomic: the compare-and-set family runs as versioned Lua
//! scripts (`crates/gs-ephemeral/lua/`, invoked with `EVALSHA` and reloaded on `NOSCRIPT`), the
//! rest as `MULTI` pipelines. Every command has an explicit timeout
//! (`GS_REDIS_COMMAND_TIMEOUT_MS`); exceeding it or losing the connection reports
//! [`EphemeralError::Unavailable`] and flips [`EphemeralStore::is_ready`] to `false` until a
//! command succeeds again (docs/04 §7.6).

use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use redis::aio::{ConnectionManager, ConnectionManagerConfig};
use redis::{AsyncCommands, RedisError, Script};

use crate::{CheckpointCas, EphemeralError, EphemeralStore, SequenceCas};

/// TTL of `{p}:seq:{session}:{device}`, renewed on every write (docs/11 §3: 7 days).
pub const SEQUENCE_TTL: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// TTL of `{p}:devseq:{device}`, renewed on every write (docs/11 §3: 30 days).
pub const DEVICE_SEQUENCE_TTL: Duration = Duration::from_secs(30 * 24 * 60 * 60);

/// TTL of `{p}:mver:{session}`, renewed on every increment.
///
/// docs/11 §3 asks for "session lifetime + 1 day". The store does not know the lifetime of the
/// session, so it uses the upper bound of `max_session_ttl_seconds` (docs/17 §4: 31 536 000 s)
/// plus one day. Every session mutation increments the version and renews the TTL, so the key
/// always outlives its session; letting it expire earlier would restart the counter and could
/// make a stale authorization cache entry look current (ADR-0009).
pub const MEMBERSHIP_VERSION_TTL: Duration = Duration::from_secs(31_536_000 + 24 * 60 * 60);

/// Field of `{p}:trk:{device}` that holds the encoded checkpoint (docs/22 §2.1).
const CHECKPOINT_STATE_FIELD: &str = "state";

/// Field of `{p}:trk:{device}` that holds the checkpoint version.
const CHECKPOINT_VERSION_FIELD: &str = "version";

/// Redis/Valkey ephemeral store shared by every instance (ADR-0010).
pub struct RedisEphemeralStore {
    connection: ConnectionManager,
    command_timeout: Duration,
    ready: AtomicBool,
    seq_cas: Script,
    devseq_max: Script,
    presence_release: Script,
    track_cas: Script,
}

impl std::fmt::Debug for RedisEphemeralStore {
    // The connection carries the URL with credentials: never print it (docs/10 §8.2).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RedisEphemeralStore")
            .field("command_timeout", &self.command_timeout)
            .field("ready", &self.ready.load(Ordering::SeqCst))
            .finish_non_exhaustive()
    }
}

impl RedisEphemeralStore {
    /// Connects to `url` and checks the server with `PING`.
    ///
    /// `command_timeout` bounds every command, the initial connection included.
    pub async fn connect(url: &str, command_timeout: Duration) -> Result<Self, EphemeralError> {
        let client = redis::Client::open(url).map_err(|e| unavailable(&e))?;
        let config = ConnectionManagerConfig::new()
            .set_connection_timeout(Some(command_timeout))
            .set_response_timeout(Some(command_timeout));
        let connection = with_timeout(
            command_timeout,
            client.get_connection_manager_with_config(config),
        )
        .await?;
        let store = Self {
            connection,
            command_timeout,
            ready: AtomicBool::new(true),
            seq_cas: Script::new(include_str!("../lua/seq_cas.lua")),
            devseq_max: Script::new(include_str!("../lua/devseq_max.lua")),
            presence_release: Script::new(include_str!("../lua/presence_release.lua")),
            track_cas: Script::new(include_str!("../lua/track_cas.lua")),
        };
        store.ping().await?;
        Ok(store)
    }

    /// Sends `PING`; used at startup and by readiness checks.
    pub async fn ping(&self) -> Result<(), EphemeralError> {
        let mut connection = self.connection.clone();
        self.run("", async move {
            redis::cmd("PING")
                .query_async::<String>(&mut connection)
                .await
        })
        .await
        .map(|_| ())
    }

    /// Runs one command with the timeout and keeps the readiness flag in sync.
    async fn run<T>(
        &self,
        key: &str,
        operation: impl Future<Output = Result<T, RedisError>>,
    ) -> Result<T, EphemeralError> {
        match tokio::time::timeout(self.command_timeout, operation).await {
            Ok(Ok(value)) => {
                self.ready.store(true, Ordering::SeqCst);
                Ok(value)
            }
            Ok(Err(error)) if is_connection_error(&error) => {
                self.ready.store(false, Ordering::SeqCst);
                Err(unavailable(&error))
            }
            Ok(Err(error)) => Err(EphemeralError::InvalidValue {
                key: key.to_string(),
                message: format!("{:?}", error.kind()),
            }),
            Err(_elapsed) => {
                self.ready.store(false, Ordering::SeqCst);
                Err(EphemeralError::Unavailable(
                    "command timed out (GS_REDIS_COMMAND_TIMEOUT_MS)".to_string(),
                ))
            }
        }
    }
}

impl EphemeralStore for RedisEphemeralStore {
    async fn set_snapshot(
        &self,
        key: &str,
        value: &[u8],
        ttl: Duration,
    ) -> Result<(), EphemeralError> {
        let mut connection = self.connection.clone();
        let ttl_ms = millis(ttl);
        self.run(key, async move {
            connection.pset_ex::<_, _, ()>(key, value, ttl_ms).await
        })
        .await
    }

    async fn get_snapshot(&self, key: &str) -> Result<Option<Vec<u8>>, EphemeralError> {
        let mut connection = self.connection.clone();
        self.run(key, async move { connection.get(key).await })
            .await
    }

    async fn delete_snapshot(&self, key: &str) -> Result<bool, EphemeralError> {
        let mut connection = self.connection.clone();
        let deleted: u64 = self
            .run(key, async move { connection.del(key).await })
            .await?;
        Ok(deleted > 0)
    }

    async fn compare_and_set_sequence(
        &self,
        key: &str,
        current: u64,
        next: u64,
    ) -> Result<SequenceCas, EphemeralError> {
        let mut connection = self.connection.clone();
        let script = &self.seq_cas;
        let (outcome, stored): (String, String) = self
            .run(key, async move {
                script
                    .key(key)
                    .arg(current)
                    .arg(next)
                    .arg(millis(SEQUENCE_TTL))
                    .invoke_async(&mut connection)
                    .await
            })
            .await?;
        let stored = parse_decimal(key, &stored)?;
        match outcome.as_str() {
            "accepted" => Ok(SequenceCas::Accepted { previous: stored }),
            "duplicate" => Ok(SequenceCas::Duplicate { current: stored }),
            "stale" => Ok(SequenceCas::Stale { current: stored }),
            other => Err(EphemeralError::InvalidValue {
                key: key.to_string(),
                message: format!("unexpected seq_cas outcome '{other}'"),
            }),
        }
    }

    async fn set_if_greater(&self, key: &str, value: u64) -> Result<u64, EphemeralError> {
        let mut connection = self.connection.clone();
        let script = &self.devseq_max;
        let effective: String = self
            .run(key, async move {
                script
                    .key(key)
                    .arg(value)
                    .arg(millis(DEVICE_SEQUENCE_TTL))
                    .invoke_async(&mut connection)
                    .await
            })
            .await?;
        parse_decimal(key, &effective)
    }

    async fn set_presence(
        &self,
        key: &str,
        fields: &[(&str, &str)],
        ttl: Duration,
    ) -> Result<(), EphemeralError> {
        let mut connection = self.connection.clone();
        // Replace the whole hash: a reconnection must not inherit fields of the previous one.
        let mut pipeline = redis::pipe();
        pipeline.atomic().del(key).ignore();
        if !fields.is_empty() {
            pipeline.hset_multiple(key, fields).ignore();
            pipeline.pexpire(key, signed_millis(ttl)).ignore();
        }
        self.run(key, async move {
            pipeline.query_async::<()>(&mut connection).await
        })
        .await
    }

    async fn get_presence(
        &self,
        key: &str,
    ) -> Result<Option<Vec<(String, String)>>, EphemeralError> {
        let mut connection = self.connection.clone();
        let fields: Vec<(String, String)> = self
            .run(key, async move { connection.hgetall(key).await })
            .await?;
        Ok((!fields.is_empty()).then_some(fields))
    }

    async fn release_presence(
        &self,
        key: &str,
        connection_id: &str,
    ) -> Result<bool, EphemeralError> {
        let mut connection = self.connection.clone();
        let script = &self.presence_release;
        let released: u64 = self
            .run(key, async move {
                script
                    .key(key)
                    .arg(connection_id)
                    .invoke_async(&mut connection)
                    .await
            })
            .await?;
        Ok(released == 1)
    }

    async fn get_membership_version(&self, key: &str) -> Result<Option<u64>, EphemeralError> {
        self.get_counter(key).await
    }

    async fn get_counter(&self, key: &str) -> Result<Option<u64>, EphemeralError> {
        let mut connection = self.connection.clone();
        let stored: Option<String> = self
            .run(key, async move { connection.get(key).await })
            .await?;
        stored.map(|text| parse_decimal(key, &text)).transpose()
    }

    async fn increment_membership_version(&self, key: &str) -> Result<u64, EphemeralError> {
        let mut connection = self.connection.clone();
        let mut pipeline = redis::pipe();
        pipeline
            .atomic()
            .incr(key, 1)
            .pexpire(key, signed_millis(MEMBERSHIP_VERSION_TTL))
            .ignore();
        let (version,): (u64,) = self
            .run(
                key,
                async move { pipeline.query_async(&mut connection).await },
            )
            .await?;
        Ok(version)
    }

    async fn set_ticket(
        &self,
        key: &str,
        grant: &[u8],
        ttl: Duration,
    ) -> Result<(), EphemeralError> {
        let mut connection = self.connection.clone();
        let ttl_ms = millis(ttl);
        self.run(key, async move {
            connection.pset_ex::<_, _, ()>(key, grant, ttl_ms).await
        })
        .await
    }

    async fn take_ticket(&self, key: &str) -> Result<Option<Vec<u8>>, EphemeralError> {
        let mut connection = self.connection.clone();
        // Native GETDEL: single use without a script (docs/11 §3).
        self.run(key, async move { connection.get_del(key).await })
            .await
    }

    async fn increment_counter(&self, key: &str, window: Duration) -> Result<u64, EphemeralError> {
        let mut connection = self.connection.clone();
        let mut pipeline = redis::pipe();
        // `PEXPIRE … NX` starts the window on the first increment only (Redis/Valkey >= 7.0).
        pipeline
            .atomic()
            .incr(key, 1)
            .cmd("PEXPIRE")
            .arg(key)
            .arg(millis(window))
            .arg("NX")
            .ignore();
        let (count,): (u64,) = self
            .run(
                key,
                async move { pipeline.query_async(&mut connection).await },
            )
            .await?;
        Ok(count)
    }

    async fn set_checkpoint(
        &self,
        key: &str,
        value: &[u8],
        ttl: Duration,
    ) -> Result<u64, EphemeralError> {
        let mut connection = self.connection.clone();
        let mut pipeline = redis::pipe();
        pipeline
            .atomic()
            .hincr(key, CHECKPOINT_VERSION_FIELD, 1)
            .hset(key, CHECKPOINT_STATE_FIELD, value)
            .ignore()
            .pexpire(key, signed_millis(ttl))
            .ignore();
        let (version,): (u64,) = self
            .run(
                key,
                async move { pipeline.query_async(&mut connection).await },
            )
            .await?;
        Ok(version)
    }

    async fn get_checkpoint(&self, key: &str) -> Result<Option<(Vec<u8>, u64)>, EphemeralError> {
        let mut connection = self.connection.clone();
        let (state, version): (Option<Vec<u8>>, Option<String>) = self
            .run(key, async move {
                redis::cmd("HMGET")
                    .arg(key)
                    .arg(CHECKPOINT_STATE_FIELD)
                    .arg(CHECKPOINT_VERSION_FIELD)
                    .query_async(&mut connection)
                    .await
            })
            .await?;
        match (state, version) {
            (Some(state), Some(version)) => Ok(Some((state, parse_decimal(key, &version)?))),
            _ => Ok(None),
        }
    }

    async fn compare_and_set_checkpoint(
        &self,
        key: &str,
        expected_version: u64,
        value: &[u8],
        ttl: Duration,
    ) -> Result<CheckpointCas, EphemeralError> {
        let mut connection = self.connection.clone();
        let script = &self.track_cas;
        let (applied, version): (u8, String) = self
            .run(key, async move {
                script
                    .key(key)
                    .arg(expected_version)
                    .arg(value)
                    .arg(millis(ttl))
                    .invoke_async(&mut connection)
                    .await
            })
            .await?;
        let version = parse_decimal(key, &version)?;
        if applied == 1 {
            Ok(CheckpointCas::Applied { version })
        } else {
            Ok(CheckpointCas::Conflict {
                current_version: version,
            })
        }
    }

    fn is_ready(&self) -> bool {
        self.ready.load(Ordering::SeqCst)
    }
}

/// Converts a TTL to whole milliseconds, never zero: `PX 0` is rejected by the server.
fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis())
        .unwrap_or(u64::MAX)
        .max(1)
}

/// Same as [`millis`] for the commands that take a signed argument (`PEXPIRE`).
fn signed_millis(duration: Duration) -> i64 {
    i64::try_from(millis(duration)).unwrap_or(i64::MAX)
}

/// Parses a decimal counter returned by the server.
fn parse_decimal(key: &str, text: &str) -> Result<u64, EphemeralError> {
    text.parse::<u64>()
        .map_err(|e| EphemeralError::InvalidValue {
            key: key.to_string(),
            message: format!("counter is not a decimal integer: {e}"),
        })
}

/// Whether the error means the backend is unreachable rather than a bad value.
fn is_connection_error(error: &RedisError) -> bool {
    error.is_io_error()
        || error.is_timeout()
        || error.is_connection_dropped()
        || error.is_connection_refusal()
}

/// Maps a connection error without its details: they may echo the URL (docs/10 §8.2).
fn unavailable(error: &RedisError) -> EphemeralError {
    EphemeralError::Unavailable(format!("{:?}", error.kind()))
}

/// Bounds a connection attempt with the command timeout.
async fn with_timeout<T>(
    timeout: Duration,
    operation: impl Future<Output = Result<T, RedisError>>,
) -> Result<T, EphemeralError> {
    match tokio::time::timeout(timeout, operation).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(error)) => Err(unavailable(&error)),
        Err(_elapsed) => Err(EphemeralError::Unavailable(
            "connection timed out (GS_REDIS_COMMAND_TIMEOUT_MS)".to_string(),
        )),
    }
}
