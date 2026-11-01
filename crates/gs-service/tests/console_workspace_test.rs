//! Role checks of the console workspace operations against PostgreSQL (T-F15-05, FR-CON-003,
//! docs/06 §8.6): a `VIEWER` reads but never changes, an `ADMIN` manages projects and keys.
#![forbid(unsafe_code)]

use gs_domain::ids::{AccountId, TenantId};
use gs_service::console_workspace::{self as workspace, ProjectUpdate, WorkspaceError};
use gs_service::migrate::{PgPool, connect_pool, run_migrations};
use gs_service::{InMemoryBus, gs_store::ConsoleRepository};
use uuid::Uuid;

fn test_database_url() -> String {
    std::env::var("GS_DATABASE_URL").unwrap_or_else(|_| {
        "postgres://geostream:geostream_dev_password@127.0.0.1:5432/geostream_test".to_string()
    })
}

async fn pool() -> PgPool {
    let pool = connect_pool(&test_database_url()).await.expect("connect");
    run_migrations(&pool).await.expect("migrate");
    pool
}

async fn account(pool: &PgPool) -> AccountId {
    let email = format!("role.{}@example.test", Uuid::new_v4().simple());
    let mut hash = Uuid::new_v4().as_bytes().to_vec();
    hash.extend_from_slice(Uuid::new_v4().as_bytes());
    ConsoleRepository::find_or_create_account(pool, &email, &hash, true)
        .await
        .expect("create")
        .expect("account")
        .0
        .id
}

async fn add_member(pool: &PgPool, account: AccountId, org: TenantId, role: &str) {
    sqlx::query(
        "INSERT INTO tenant_memberships (account_id, tenant_id, role) VALUES ($1, $2, $3);",
    )
    .bind(account.into_uuid())
    .bind(org.into_uuid())
    .bind(role)
    .execute(pool)
    .await
    .expect("membership");
}

#[tokio::test]
async fn test_fr_con_003_a_viewer_reads_but_cannot_change_anything() {
    // covers: FR-CON-003, docs/06 §8.6
    let pool = pool().await;
    let owner = account(&pool).await;
    let viewer = account(&pool).await;
    let org = workspace::create_organization(&pool, owner, "Roles", 3)
        .await
        .expect("org")
        .tenant
        .id;
    let project = workspace::create_project(&pool, owner, org, "App", None, None)
        .await
        .expect("project")
        .id;
    add_member(&pool, viewer, org, "VIEWER").await;

    assert_eq!(
        workspace::list_projects(&pool, viewer, org)
            .await
            .expect("viewer reads")
            .len(),
        1
    );
    assert!(
        workspace::list_api_keys(&pool, viewer, org, project)
            .await
            .is_ok()
    );

    assert_eq!(
        workspace::create_project(&pool, viewer, org, "Other", None, None)
            .await
            .unwrap_err(),
        WorkspaceError::PermissionDenied
    );
    assert_eq!(
        workspace::create_api_key(
            &pool,
            viewer,
            org,
            project,
            "k",
            vec!["sessions:read".to_string()],
            None
        )
        .await
        .err(),
        Some(WorkspaceError::PermissionDenied)
    );
    let update = workspace::update_project::<InMemoryBus>(
        &pool,
        viewer,
        org,
        project,
        ProjectUpdate {
            enabled: Some(false),
            ..ProjectUpdate::default()
        },
        None,
        None,
    )
    .await;
    assert_eq!(update.unwrap_err(), WorkspaceError::PermissionDenied);
    // The project is untouched.
    let current = workspace::get_project(&pool, owner, org, project)
        .await
        .expect("get");
    assert_eq!(current.status.as_str(), "ACTIVE");
}

#[tokio::test]
async fn test_fr_con_003_an_admin_manages_projects_and_keys() {
    // covers: FR-CON-003
    let pool = pool().await;
    let owner = account(&pool).await;
    let admin = account(&pool).await;
    let org = workspace::create_organization(&pool, owner, "Admins", 3)
        .await
        .expect("org")
        .tenant
        .id;
    add_member(&pool, admin, org, "ADMIN").await;

    let project = workspace::create_project(&pool, admin, org, "By admin", None, None)
        .await
        .expect("admin creates")
        .id;
    let key = workspace::create_api_key(
        &pool,
        admin,
        org,
        project,
        "ci",
        vec!["sessions:read".to_string()],
        Some(30),
    )
    .await
    .expect("admin issues a key");
    assert!(key.key.expires_at.is_some());
    workspace::revoke_api_key::<InMemoryBus>(&pool, admin, org, project, key.key.id, None, None)
        .await
        .expect("admin revokes");
}

#[tokio::test]
async fn test_a_stranger_gets_not_found_not_permission_denied() {
    // covers: docs/06 §8.6 (no oracle on other people's organizations)
    let pool = pool().await;
    let owner = account(&pool).await;
    let stranger = account(&pool).await;
    let org = workspace::create_organization(&pool, owner, "Hidden", 3)
        .await
        .expect("org")
        .tenant
        .id;
    assert_eq!(
        workspace::list_projects(&pool, stranger, org)
            .await
            .unwrap_err(),
        WorkspaceError::NotFound
    );
    assert_eq!(
        workspace::create_project(&pool, stranger, org, "x", None, None)
            .await
            .unwrap_err(),
        WorkspaceError::NotFound
    );
}
