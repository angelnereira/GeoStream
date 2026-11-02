//! Background jobs: expiry, retention and webhook delivery (`docs/11` §5).
//!
//! Every job is safe to run on several instances at once: the expiry sweep claims rows with
//! `FOR UPDATE SKIP LOCKED` and the purges are plain deletes of rows that can no longer be used.
//! Webhook delivery and the history jobs arrive with their own phases (F8, E6).
#![forbid(unsafe_code)]

use std::time::Duration;

use gs_service::gs_store::repos::{IdempotencyRepository, RetentionRepository};
use gs_service::gs_store::{ConsoleRepository, PgPool};
use gs_service::{BusBackend, EphemeralBackend, expire_due_sessions_with_bus};
use std::time::Instant;

use time::OffsetDateTime;
use tracing::{info, warn};

/// Records one run of a job: its duration, the rows it handled and whether it failed. The label is
/// the job name only (`docs/16` §4.2): never an identifier.
fn record_job(job: &'static str, started: Instant, rows: u64, failed: bool) {
    metrics::histogram!("gs_worker_job_duration_seconds", "job" => job)
        .record(started.elapsed().as_secs_f64());
    metrics::counter!("gs_worker_job_rows_total", "job" => job).increment(rows);
    if failed {
        metrics::counter!("gs_worker_job_failures_total", "job" => job).increment(1);
    }
}

/// How often the maintenance purges run (`docs/11` §5: idempotency cleanup is hourly).
pub const MAINTENANCE_INTERVAL: Duration = Duration::from_secs(3600);

/// Settings of the worker, read from `docs/17` §2.
#[derive(Debug, Clone, Copy)]
pub struct WorkerConfig {
    /// `GS_WORKER_EXPIRY_INTERVAL_S`.
    pub expiry_interval: Duration,
    /// `GS_WORKER_BATCH_SIZE`.
    pub batch_size: u32,
    /// `GS_WORKER_RETENTION_HOUR_UTC`: hour of the day (UTC) from which the daily retention runs.
    pub retention_hour_utc: u8,
    /// `GS_RETENTION_CLOSED_SESSIONS_D`.
    pub retention_closed_sessions: Duration,
    /// `GS_RETENTION_INVITATIONS_D`.
    pub retention_invitations: Duration,
}

impl Default for WorkerConfig {
    /// Defaults of `docs/17` §2.
    fn default() -> Self {
        Self {
            expiry_interval: Duration::from_secs(15),
            batch_size: 500,
            retention_hour_utc: 3,
            retention_closed_sessions: Duration::from_secs(90 * 86_400),
            retention_invitations: Duration::from_secs(30 * 86_400),
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
    let started = Instant::now();
    match expire_due_sessions_with_bus(pool, ephemeral, bus, key_prefix, batch_size).await {
        Ok(expired) => {
            if !expired.is_empty() {
                info!(count = expired.len(), "sessions expired");
            }
            record_job("session_expiry", started, expired.len() as u64, false);
            expired.len()
        }
        Err(error) => {
            warn!(error = %error, "session expiry sweep failed");
            record_job("session_expiry", started, 0, true);
            0
        }
    }
}

/// Removes what has expired and can no longer be used: idempotency records and the console's
/// codes, sessions and counters. Each purge is independent, so one failing does not stop the other.
pub async fn run_maintenance_once(pool: &PgPool) -> MaintenanceReport {
    let mut report = MaintenanceReport::default();
    let started = Instant::now();
    match IdempotencyRepository::purge_expired(pool, OffsetDateTime::now_utc()).await {
        Ok(removed) => {
            report.idempotency_records = removed;
            record_job("idempotency_purge", started, removed, false);
        }
        Err(_) => {
            warn!("idempotency purge failed");
            record_job("idempotency_purge", started, 0, true);
        }
    }
    let started = Instant::now();
    match ConsoleRepository::purge_expired(pool).await {
        Ok(removed) => {
            report.console_rows = removed;
            record_job("console_purge", started, removed, false);
        }
        Err(_) => {
            warn!("console purge failed");
            record_job("console_purge", started, 0, true);
        }
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

/// What one retention run removed. Counts only (`docs/10` §8.2).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct RetentionReport {
    /// Closed or expired sessions past their retention (with their memberships and invitations).
    pub ended_sessions: u64,
    /// Expired or revoked invitations past their retention.
    pub spent_invitations: u64,
}

/// Deletes, in batches of `config.batch_size`, the sessions that ended and the invitations that were
/// spent longer ago than their retention (`docs/17` §2: 90 and 30 days by default). Active sessions
/// are never deleted. Safe with several instances: every batch skips rows another one holds.
pub async fn run_retention_once(pool: &PgPool, config: &WorkerConfig) -> RetentionReport {
    let now = OffsetDateTime::now_utc();
    let batch = i64::from(config.batch_size);
    let mut report = RetentionReport::default();

    let started = Instant::now();
    let cutoff = now - config.retention_closed_sessions;
    let mut failed = false;
    loop {
        match RetentionRepository::purge_ended_sessions(pool, cutoff, batch).await {
            Ok(removed) => {
                report.ended_sessions += removed;
                if removed < batch as u64 {
                    break;
                }
            }
            Err(_) => {
                warn!("ended sessions purge failed");
                failed = true;
                break;
            }
        }
    }
    record_job(
        "ended_sessions_purge",
        started,
        report.ended_sessions,
        failed,
    );

    let started = Instant::now();
    let cutoff = now - config.retention_invitations;
    let mut failed = false;
    loop {
        match RetentionRepository::purge_spent_invitations(pool, cutoff, batch).await {
            Ok(removed) => {
                report.spent_invitations += removed;
                if removed < batch as u64 {
                    break;
                }
            }
            Err(_) => {
                warn!("spent invitations purge failed");
                failed = true;
                break;
            }
        }
    }
    record_job(
        "spent_invitations_purge",
        started,
        report.spent_invitations,
        failed,
    );

    if report.ended_sessions + report.spent_invitations > 0 {
        info!(
            ended_sessions = report.ended_sessions,
            spent_invitations = report.spent_invitations,
            "retention purge completed"
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
    let retention = async {
        // Checked every ten minutes: it runs once per UTC day, from `retention_hour_utc` on. A restart
        // after that hour runs it again, which is harmless (the purge is idempotent).
        let mut tick = tokio::time::interval(Duration::from_secs(600));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut last_run: Option<time::Date> = None;
        loop {
            tick.tick().await;
            let now = OffsetDateTime::now_utc();
            if now.hour() >= config.retention_hour_utc && last_run != Some(now.date()) {
                last_run = Some(now.date());
                run_retention_once(&pool, &config).await;
            }
        }
    };
    tokio::join!(expiry, maintenance, retention);
}
