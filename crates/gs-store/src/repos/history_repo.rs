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
