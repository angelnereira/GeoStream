//! Background jobs: expiry, retention and webhook delivery (`docs/11` §5).
//!
//! Every job is safe to run on several instances at once: the expiry sweep claims rows with
//! `FOR UPDATE SKIP LOCKED` and the purges are plain deletes of rows that can no longer be used.
//! Webhook delivery and the history jobs arrive with their own phases (F8, E6).
#![forbid(unsafe_code)]

use std::time::Duration;

use gs_service::gs_store::repos::IdempotencyRepository;
use gs_service::gs_store::{ConsoleRepository, PgPool};
use gs_service::{BusBackend, EphemeralBackend, expire_due_sessions_with_bus};
use time::OffsetDateTime;
use tracing::{info, warn};

/// How often the maintenance purges run (`docs/11` §5: idempotency cleanup is hourly).
pub const MAINTENANCE_INTERVAL: Duration = Duration::from_secs(3600);

/// Settings of the worker, read from `docs/17` §2.
#[derive(Debug, Clone, Copy)]
pub struct WorkerConfig {
    /// `GS_WORKER_EXPIRY_INTERVAL_S`.
    pub expiry_interval: Duration,
    /// `GS_WORKER_BATCH_SIZE`.
    pub batch_size: u32,
}

impl Default for WorkerConfig {
    /// Defaults of `docs/17` §2.
    fn default() -> Self {
        Self {
            expiry_interval: Duration::from_secs(15),
            batch_size: 500,
        }
    }
}

/// What one maintenance run removed. Counts only: never identifiers (`docs/10` §8.2).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct MaintenanceReport {
    /// Idempotency records past their `expires_at`.
    pub idempotency_records: u64,
    /// Console one-time codes, sessions and e-mail counters that can no longer be used.
    pub console_rows: u64,
}

/// Marks the sessions past their expiry as `EXPIRED`, clears their ephemeral state and tells the
/// connected instances. Returns how many were expired. A failure is logged and counted as zero:
/// the next sweep retries.
pub async fn expire_sessions_once(
    pool: &PgPool,
    ephemeral: &EphemeralBackend,
    bus: &BusBackend,
    key_prefix: &str,
    batch_size: u32,
) -> usize {
    match expire_due_sessions_with_bus(pool, ephemeral, bus, key_prefix, batch_size).await {
        Ok(expired) => {
            if !expired.is_empty() {
                info!(count = expired.len(), "sessions expired");
            }
            expired.len()
        }
        Err(error) => {
            warn!(error = %error, "session expiry sweep failed");
            0
        }
    }
}

/// Removes what has expired and can no longer be used: idempotency records and the console's
/// codes, sessions and counters. Each purge is independent, so one failing does not stop the other.
pub async fn run_maintenance_once(pool: &PgPool) -> MaintenanceReport {
    let mut report = MaintenanceReport::default();
    match IdempotencyRepository::purge_expired(pool, OffsetDateTime::now_utc()).await {
        Ok(removed) => report.idempotency_records = removed,
        Err(_) => warn!("idempotency purge failed"),
    }
    match ConsoleRepository::purge_expired(pool).await {
        Ok(removed) => report.console_rows = removed,
        Err(_) => warn!("console purge failed"),
    }
    if report.idempotency_records + report.console_rows > 0 {
        info!(
            idempotency_records = report.idempotency_records,
            console_rows = report.console_rows,
            "maintenance purge completed"
        );
    }
    report
}

/// Runs the worker jobs until the task is dropped or aborted.
pub async fn run(
    pool: PgPool,
    ephemeral: std::sync::Arc<EphemeralBackend>,
    bus: std::sync::Arc<BusBackend>,
    key_prefix: String,
    config: WorkerConfig,
) {
    info!(
        expiry_interval_s = config.expiry_interval.as_secs(),
        batch_size = config.batch_size,
        "worker started"
    );
    let expiry = async {
        let mut tick = tokio::time::interval(config.expiry_interval);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            expire_sessions_once(&pool, &ephemeral, &bus, &key_prefix, config.batch_size).await;
        }
    };
    let maintenance = async {
        let mut tick = tokio::time::interval(MAINTENANCE_INTERVAL);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            run_maintenance_once(&pool).await;
        }
    };
    tokio::join!(expiry, maintenance);
}
