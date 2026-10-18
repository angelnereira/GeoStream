//! History and k-anonymous aggregation against a live PostgreSQL (E6).
#![forbid(unsafe_code)]

use gs_store::PgPool;
use gs_store::migrate::{connect_pool, run_migrations};
use gs_store::repos::{CellRow, HistoryRepository, LocationPointRow};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

fn test_database_url() -> String {
    std::env::var("GS_DATABASE_URL").unwrap_or_else(|_| {
        "postgres://geostream:geostream_dev_password@127.0.0.1:5432/geostream_test".to_string()
    })
}

async fn setup_db() -> PgPool {
    let url = test_database_url();
    let pool = connect_pool(&url).await.expect("connect to test database");
    run_migrations(&pool).await.expect("apply migrations");
    pool
}

async fn create_tenant(pool: &PgPool) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO tenants (id, name, status, policy) VALUES ($1, $2, 'ACTIVE', '{}'::jsonb);",
    )
    .bind(id)
    .bind(format!("HistTenant-{}", &id.to_string()[..8]))
    .execute(pool)
    .await
    .expect("insert tenant");
    id
}

fn point(tenant: Uuid, captured_at: OffsetDateTime, lat: f64, lon: f64) -> LocationPointRow {
    LocationPointRow {
        id: Uuid::now_v7(),
        tenant_id: gs_domain::ids::TenantId::from_uuid(tenant),
        session_id: Uuid::now_v7(),
        member_id: Uuid::now_v7(),
        subject_id: Uuid::now_v7(),
        device_id: Uuid::now_v7(),
        sequence: 1,
        captured_at,
        received_at: captured_at,
        latitude: lat,
        longitude: lon,
        accuracy_m: 8.0,
        speed_mps: Some(4.0),
        heading_deg: Some(90.0),
        motion: Some("AUTOMOTIVE".to_string()),
        is_mock: false,
        h3_cell: 6_177_000_000_000_000_000i64,
    }
}

#[tokio::test]
async fn history_points_round_trip_through_a_month_partition() {
    // covers: FR-HIS-001, docs/24 §2, docs/11 §2.0
    let pool = setup_db().await;
    let tenant = create_tenant(&pool).await;
    let captured = OffsetDateTime::from_unix_timestamp(1_768_435_200).unwrap(); // 2026-01-15
    HistoryRepository::ensure_month_partition(&pool, captured)
        .await
        .expect("create partition");

    let rows = vec![
        point(tenant, captured, 8.9824, -79.5199),
        point(tenant, captured + Duration::seconds(5), 8.9830, -79.5199),
    ];
    let inserted = HistoryRepository::insert_points(&pool, &rows)
        .await
        .expect("insert points");
    assert_eq!(inserted, 2);

    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM location_points WHERE tenant_id = $1")
            .bind(tenant)
            .fetch_one(&pool)
            .await
            .expect("count");
    assert_eq!(count, 2);
}

#[tokio::test]
async fn a_cell_below_k_is_suppressed_and_never_stored() {
    // covers: FR-ANA-001, NFR-PRIV-003, ANA-KAN-001/002, docs/24 §6
    let pool = setup_db().await;
    let tenant = create_tenant(&pool).await;
    let bucket = OffsetDateTime::from_unix_timestamp(1_768_435_200).unwrap();
    let cell = 6_177_000_000_000_000_000i64;

    // 4 distinct subjects, k = 5: suppressed, no row written.
    let below = CellRow {
        h3_cell: cell,
        bucket_start: bucket,
        distinct_subjects: 4,
        point_count: 40,
        stop_count: 0,
    };
    let stored = HistoryRepository::upsert_cell(
        &pool,
        gs_domain::ids::TenantId::from_uuid(tenant),
        &below,
        5,
    )
    .await
    .expect("upsert below k");
    assert!(!stored, "a cell below k must not be stored");

    // 5 distinct subjects: published.
    let above = CellRow {
        distinct_subjects: 5,
        ..below.clone()
    };
    let stored = HistoryRepository::upsert_cell(
        &pool,
        gs_domain::ids::TenantId::from_uuid(tenant),
        &above,
        5,
    )
    .await
    .expect("upsert above k");
    assert!(stored);

    let cells = HistoryRepository::read_cells(
        &pool,
        gs_domain::ids::TenantId::from_uuid(tenant),
        bucket - Duration::hours(1),
        bucket + Duration::hours(1),
    )
    .await
    .expect("read cells");
    assert_eq!(
        cells.len(),
        1,
        "only the cell that reached k is ever visible"
    );
    assert_eq!(cells[0].distinct_subjects, 5);
}
