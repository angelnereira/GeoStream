//! Tag repository against a live PostgreSQL (E8).
#![forbid(unsafe_code)]

use gs_store::PgPool;
use gs_store::migrate::{connect_pool, run_migrations};
use gs_store::repos::{Tag, TagRepository};
use time::OffsetDateTime;
use uuid::Uuid;

fn test_database_url() -> String {
    std::env::var("GS_DATABASE_URL").unwrap_or_else(|_| {
        "postgres://geostream:geostream_dev_password@127.0.0.1:5432/geostream_test".to_string()
    })
}

async fn setup_db() -> PgPool {
    let url = test_database_url();
    let pool = connect_pool(&url).await.expect("connect");
    run_migrations(&pool).await.expect("migrate");
    pool
}

#[tokio::test]
async fn tag_round_trip_and_state_change() {
    // covers: FR-EDGE-001, docs/25 §2.2, INV-EDGE-1
    let pool = setup_db().await;
    let tenant_uuid = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO tenants (id, name, status, policy) VALUES ($1,$2,'ACTIVE','{}'::jsonb);",
    )
    .bind(tenant_uuid)
    .bind(format!("TagTenant-{}", &tenant_uuid.to_string()[..8]))
    .execute(&pool)
    .await
    .expect("tenant");

    let tenant_id = gs_domain::ids::TenantId::from_uuid(tenant_uuid);
    let owner = gs_domain::ids::SubjectId::new();
    let now = OffsetDateTime::now_utc();
    let tag = Tag {
        id: gs_domain::ids::TagId::new(),
        tenant_id,
        owner_subject_id: owner,
        model: "lora-sensor-a".to_string(),
        name: Some("bici".to_string()),
        state: "ACTIVE".to_string(),
        separated_at: None,
        created_at: now,
        updated_at: now,
    };
    TagRepository::create(&pool, &tag).await.expect("create");

    let fetched = TagRepository::get(&pool, tenant_id, tag.id)
        .await
        .expect("get")
        .expect("present");
    assert_eq!(fetched.model, "lora-sensor-a");
    assert_eq!(fetched.state, "ACTIVE");
    assert!(fetched.id.to_string().starts_with("tag_"));

    // The schema has no `EIK` column at all: it cannot be stored even by mistake.
    let eik_columns: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM information_schema.columns \
         WHERE table_name = 'tags' AND column_name ~* 'eik|encrypted_identity';",
    )
    .fetch_one(&pool)
    .await
    .expect("columns");
    assert_eq!(eik_columns, 0);

    // SEPARATED sets the timestamp and is visible afterwards.
    let changed = TagRepository::set_state(&pool, tenant_id, tag.id, "SEPARATED", Some(now), now)
        .await
        .expect("set state");
    assert!(changed);
    let fetched = TagRepository::get(&pool, tenant_id, tag.id)
        .await
        .expect("get")
        .unwrap();
    assert_eq!(fetched.state, "SEPARATED");
    assert!(fetched.separated_at.is_some());
}
