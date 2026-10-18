//! History repository: `location_points` and the k-anonymous aggregate
//! `h3_activity_hourly` (`docs/24`, `docs/11` §2.0, E6).
#![forbid(unsafe_code)]

use sqlx::{PgPool, Row};
use time::OffsetDateTime;
use uuid::Uuid;

use gs_domain::ids::TenantId;

use crate::error::StoreError;

/// One row of `location_points`.
#[derive(Debug, Clone)]
pub struct LocationPointRow {
    pub id: Uuid,
    pub tenant_id: TenantId,
    pub session_id: Uuid,
    pub member_id: Uuid,
    pub subject_id: Uuid,
    pub device_id: Uuid,
    pub sequence: i64,
    pub captured_at: OffsetDateTime,
    pub received_at: OffsetDateTime,
    pub latitude: f64,
    pub longitude: f64,
    pub accuracy_m: f64,
    pub speed_mps: Option<f64>,
    pub heading_deg: Option<f64>,
    pub motion: Option<String>,
    pub is_mock: bool,
    /// H3 cell as an opaque integer (ADR-0026). Never an identifier.
    pub h3_cell: i64,
}

/// One aggregated cell of `h3_activity_hourly`.
#[derive(Debug, Clone)]
pub struct CellRow {
    pub h3_cell: i64,
    pub bucket_start: OffsetDateTime,
    pub distinct_subjects: i32,
    pub point_count: i32,
    pub stop_count: i32,
}

pub struct HistoryRepository;

impl HistoryRepository {
    /// Creates the monthly partition that covers `instant`, if missing.
    ///
    /// The worker calls this ahead of time (`GS_HISTORY_PARTITION_LOOKAHEAD_DAYS`,
    /// `docs/11` §2.0). It is idempotent.
    pub async fn ensure_month_partition(
        pool: &PgPool,
        instant: OffsetDateTime,
    ) -> Result<(), StoreError> {
        let (year, month) = (instant.year(), instant.month() as u8);
        let (next_year, next_month) = if month == 12 {
            (year + 1, 1)
        } else {
            (year, month + 1)
        };
        let name = format!("location_points_{year}_{month:02}");
        let sql = format!(
            "CREATE TABLE IF NOT EXISTS {name} PARTITION OF location_points \
             FOR VALUES FROM ('{year}-{month:02}-01 00:00:00+00') \
             TO ('{next_year}-{next_month:02}-01 00:00:00+00');"
        );
        // The SQL is assembled from integers (year/month), never from user
        // input, so it carries no injection surface; `AssertSqlSafe` records
        // that audit explicitly (sqlx 0.9 `SqlSafeStr`).
        sqlx::query(sqlx::AssertSqlSafe(sql))
            .execute(pool)
            .await
            .map_err(StoreError::Database)?;
        Ok(())
    }

    /// Inserts a batch of points in one transaction.
    pub async fn insert_points(
        pool: &PgPool,
        rows: &[LocationPointRow],
    ) -> Result<u64, StoreError> {
        if rows.is_empty() {
            return Ok(0);
        }
        let mut tx = pool.begin().await.map_err(StoreError::Database)?;
        for r in rows {
            sqlx::query(
                r#"
                INSERT INTO location_points (
                    id, tenant_id, session_id, member_id, subject_id, device_id,
                    sequence, captured_at, received_at, latitude, longitude,
                    accuracy_m, speed_mps, heading_deg, motion, is_mock, h3_cell
                )
                VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17);
                "#,
            )
            .bind(r.id)
            .bind(r.tenant_id.into_uuid())
            .bind(r.session_id)
            .bind(r.member_id)
            .bind(r.subject_id)
            .bind(r.device_id)
            .bind(r.sequence)
            .bind(r.captured_at)
            .bind(r.received_at)
            .bind(r.latitude)
            .bind(r.longitude)
            .bind(r.accuracy_m)
            .bind(r.speed_mps)
            .bind(r.heading_deg)
            .bind(&r.motion)
            .bind(r.is_mock)
            .bind(r.h3_cell)
            .execute(&mut *tx)
            .await
            .map_err(StoreError::Database)?;
        }
        tx.commit().await.map_err(StoreError::Database)?;
        Ok(rows.len() as u64)
    }

    /// Upserts an aggregated cell, **suppressing it when it is below `k`**.
    ///
    /// The suppression happens here, at aggregation time, not in the query: a
    /// cell that never reaches `k` is never stored, so it cannot leak through a
    /// later query, an export or a backup (`ANA-KAN-003`, `NFR-PRIV-003`).
    /// Returns `true` when the cell was stored, `false` when suppressed.
    pub async fn upsert_cell(
        pool: &PgPool,
        tenant_id: TenantId,
        cell: &CellRow,
        k: i32,
    ) -> Result<bool, StoreError> {
        if cell.distinct_subjects < k {
            return Ok(false);
        }
        sqlx::query(
            r#"
            INSERT INTO h3_activity_hourly (
                tenant_id, h3_cell, bucket_start, distinct_subjects, point_count, stop_count
            )
            VALUES ($1,$2,$3,$4,$5,$6)
            ON CONFLICT (tenant_id, h3_cell, bucket_start) DO UPDATE
              SET distinct_subjects = EXCLUDED.distinct_subjects,
                  point_count = EXCLUDED.point_count,
                  stop_count = EXCLUDED.stop_count;
            "#,
        )
        .bind(tenant_id.into_uuid())
        .bind(cell.h3_cell)
        .bind(cell.bucket_start)
        .bind(cell.distinct_subjects)
        .bind(cell.point_count)
        .bind(cell.stop_count)
        .execute(pool)
        .await
        .map_err(StoreError::Database)?;
        Ok(true)
    }

    /// Reads the cells of a tenant inside a window (only stored, non-suppressed
    /// cells are ever returned).
    pub async fn read_cells(
        pool: &PgPool,
        tenant_id: TenantId,
        from: OffsetDateTime,
        to: OffsetDateTime,
    ) -> Result<Vec<CellRow>, StoreError> {
        let rows = sqlx::query(
            r#"
            SELECT h3_cell, bucket_start, distinct_subjects, point_count, stop_count
            FROM h3_activity_hourly
            WHERE tenant_id = $1 AND bucket_start >= $2 AND bucket_start < $3
            ORDER BY bucket_start ASC, h3_cell ASC;
            "#,
        )
        .bind(tenant_id.into_uuid())
        .bind(from)
        .bind(to)
        .fetch_all(pool)
        .await
        .map_err(StoreError::Database)?;

        Ok(rows
            .into_iter()
            .map(|r| CellRow {
                h3_cell: r.get("h3_cell"),
                bucket_start: r.get("bucket_start"),
                distinct_subjects: r.get("distinct_subjects"),
                point_count: r.get("point_count"),
                stop_count: r.get("stop_count"),
            })
            .collect())
    }
}

impl HistoryRepository {
    /// Drops every monthly partition of `location_points` strictly older than
    /// `(year, month)`. Retention by partition drop is O(1) per partition and
    /// leaves no row behind (`docs/24` §5, `docs/11` §6).
    ///
    /// Returns the names dropped.
    pub async fn drop_partitions_before(
        pool: &PgPool,
        year: i32,
        month: u8,
    ) -> Result<Vec<String>, StoreError> {
        let rows = sqlx::query(
            r#"
            SELECT c.relname AS name
            FROM pg_inherits i
            JOIN pg_class c ON c.oid = i.inhrelid
            JOIN pg_class p ON p.oid = i.inhparent
            WHERE p.relname = 'location_points';
            "#,
        )
        .fetch_all(pool)
        .await
        .map_err(StoreError::Database)?;

        let mut dropped = Vec::new();
        for row in rows {
            let name: String = row.get("name");
            let Some((py, pm)) = parse_partition_suffix(&name) else {
                continue;
            };
            if (py, pm) < (year, month) {
                // The name comes from `pg_class` and matches the strict
                // `location_points_YYYY_MM` shape, so it is a known identifier,
                // not user input; `AssertSqlSafe` records that audit.
                let sql = format!("DROP TABLE IF EXISTS {name};");
                sqlx::query(sqlx::AssertSqlSafe(sql))
                    .execute(pool)
                    .await
                    .map_err(StoreError::Database)?;
                dropped.push(name);
            }
        }
        Ok(dropped)
    }

    /// Erases every history row of one subject (`FR-HIS-005`, `NFR-PRIV-002`).
    ///
    /// Returns the number of `location_points` rows removed. The caller runs it
    /// inside the subject-erasure flow; it never touches live snapshots (there
    /// are none in PostgreSQL).
    pub async fn purge_subject(
        pool: &PgPool,
        tenant_id: TenantId,
        subject_id: Uuid,
    ) -> Result<u64, StoreError> {
        let mut tx = pool.begin().await.map_err(StoreError::Database)?;
        // `trip_tracks`/`stops` go first: they cascade from `trips`, but being
        // explicit keeps the order readable.
        sqlx::query("DELETE FROM stops WHERE tenant_id = $1 AND member_id IN (SELECT id FROM memberships WHERE tenant_id = $1 AND subject_id = $2);")
            .bind(tenant_id.into_uuid())
            .bind(subject_id)
            .execute(&mut *tx)
            .await
            .map_err(StoreError::Database)?;
        sqlx::query("DELETE FROM trip_tracks WHERE trip_id IN (SELECT id FROM trips WHERE tenant_id = $1 AND subject_id = $2);")
            .bind(tenant_id.into_uuid())
            .bind(subject_id)
            .execute(&mut *tx)
            .await
            .map_err(StoreError::Database)?;
        sqlx::query("DELETE FROM trips WHERE tenant_id = $1 AND subject_id = $2;")
            .bind(tenant_id.into_uuid())
            .bind(subject_id)
            .execute(&mut *tx)
            .await
            .map_err(StoreError::Database)?;
        let removed =
            sqlx::query("DELETE FROM location_points WHERE tenant_id = $1 AND subject_id = $2;")
                .bind(tenant_id.into_uuid())
                .bind(subject_id)
                .execute(&mut *tx)
                .await
                .map_err(StoreError::Database)?
                .rows_affected();
        tx.commit().await.map_err(StoreError::Database)?;
        Ok(removed)
    }
}

/// Parses `location_points_YYYY_MM` into `(year, month)`.
fn parse_partition_suffix(name: &str) -> Option<(i32, u8)> {
    let suffix = name.strip_prefix("location_points_")?;
    let (year, month) = suffix.split_once('_')?;
    let year: i32 = year.parse().ok()?;
    let month: u8 = month.parse().ok()?;
    if !(1..=12).contains(&month) {
        return None;
    }
    Some((year, month))
}

/// One point of a trace query.
#[derive(Debug, Clone)]
pub struct TracePoint {
    pub latitude: f64,
    pub longitude: f64,
    pub captured_at: OffsetDateTime,
}

impl HistoryRepository {
    /// Reads the ordered points of a session inside a window.
    ///
    /// Ordering is by `captured_at` then `sequence`: a trace is a path, and the
    /// path is the order the points were captured, not the order they arrived.
    pub async fn read_points(
        pool: &PgPool,
        tenant_id: TenantId,
        session_id: Uuid,
        from: OffsetDateTime,
        to: OffsetDateTime,
        limit: i64,
    ) -> Result<Vec<TracePoint>, StoreError> {
        let rows = sqlx::query(
            r#"
            SELECT latitude, longitude, captured_at
            FROM location_points
            WHERE tenant_id = $1 AND session_id = $2
              AND captured_at >= $3 AND captured_at < $4
            ORDER BY captured_at ASC, sequence ASC
            LIMIT $5;
            "#,
        )
        .bind(tenant_id.into_uuid())
        .bind(session_id)
        .bind(from)
        .bind(to)
        .bind(limit)
        .fetch_all(pool)
        .await
        .map_err(StoreError::Database)?;
        Ok(rows
            .into_iter()
            .map(|r| TracePoint {
                latitude: r.get("latitude"),
                longitude: r.get("longitude"),
                captured_at: r.get("captured_at"),
            })
            .collect())
    }
}
