//! Console accounts, one-time codes, sessions and memberships against PostgreSQL
//! (T-F15-16, FR-CON-001, FR-CON-009, FR-CON-010, ADR-0036).
//!
//! What matters here is what must hold **under concurrency**: attempts, single use, the cooldown
//! and the daily e-mail budget are each one atomic statement, and these tests fire many requests
//! at once to prove it.
#![forbid(unsafe_code)]

use std::time::Duration;

use serde_json::json;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

use gs_domain::ids::{AccountId, TenantId};
use gs_store::{
    AccountStatus, ConsoleRepository, ConsoleRole, OtpIssue, Tenant, TenantStatus, connect_pool,
    run_migrations,
};

fn test_database_url() -> String {
    std::env::var("GS_DATABASE_URL").unwrap_or_else(|_| {
        "postgres://geostream:geostream_dev_password@127.0.0.1:5432/geostream_test".to_string()
    })
}

async fn setup_db() -> PgPool {
    let pool = connect_pool(&test_database_url())
        .await
        .expect("connect to test database");
    run_migrations(&pool).await.expect("apply migrations");
    pool
}

/// 32 random bytes, standing in for an HMAC or a SHA-256.
fn digest() -> Vec<u8> {
    [Uuid::new_v4().into_bytes(), Uuid::new_v4().into_bytes()].concat()
}

fn email() -> String {
    format!("user-{}@example.test", Uuid::new_v4().simple())
}

async fn account(pool: &PgPool) -> gs_store::ConsoleAccount {
    ConsoleRepository::find_or_create_account(pool, &email(), &digest(), true)
        .await
        .expect("create account")
        .expect("terms accepted")
        .0
}

fn tenant(name: &str) -> Tenant {
    let now = OffsetDateTime::now_utc();
    Tenant {
        id: TenantId::new(),
        name: name.to_string(),
        status: TenantStatus::Active,
        policy: json!({}),
        created_at: now,
        updated_at: now,
    }
}

#[tokio::test]
async fn test_fr_con_009_cooldown_blocks_a_second_code_and_expires() {
    // covers: FR-CON-009, ADR-0036 §2
    let pool = setup_db().await;
    let hash = digest();
    assert_eq!(
        ConsoleRepository::issue_otp(&pool, &hash, &digest(), 600.0, 3600.0)
            .await
            .unwrap(),
        OtpIssue::Issued
    );
    assert_eq!(
        ConsoleRepository::issue_otp(&pool, &hash, &digest(), 600.0, 3600.0)
            .await
            .unwrap(),
        OtpIssue::CoolingDown,
        "a second code inside the cooldown is not stored"
    );
    // With no cooldown the live code is replaced and its attempts reset.
    assert_eq!(
        ConsoleRepository::issue_otp(&pool, &hash, &digest(), 600.0, 0.0)
            .await
            .unwrap(),
        OtpIssue::Issued
    );
}

#[tokio::test]
async fn test_fr_con_009_concurrent_requests_pass_the_cooldown_only_once() {
    // covers: FR-CON-009
    let pool = setup_db().await;
    let hash = digest();
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..16 {
        let pool = pool.clone();
        let hash = hash.clone();
        tasks.spawn(async move {
            ConsoleRepository::issue_otp(&pool, &hash, &digest(), 600.0, 3600.0)
                .await
                .unwrap()
        });
    }
    let mut issued = 0;
    while let Some(result) = tasks.join_next().await {
        if result.unwrap() == OtpIssue::Issued {
            issued += 1;
        }
    }
    assert_eq!(issued, 1, "exactly one request may send the e-mail");
}

#[tokio::test]
async fn test_fr_con_001_attempts_are_counted_before_comparing_and_run_out() {
    // covers: FR-CON-001, 10 T-41
    let pool = setup_db().await;
    let hash = digest();
    let code = digest();
    ConsoleRepository::issue_otp(&pool, &hash, &code, 600.0, 0.0)
        .await
        .unwrap();
    for expected in 1..=3 {
        let attempt = ConsoleRepository::take_otp_attempt(&pool, &hash, 3)
            .await
            .unwrap()
            .expect("attempt available");
        assert_eq!(attempt.attempts, expected);
        assert_eq!(attempt.code_hmac, code);
    }
    assert!(
        ConsoleRepository::take_otp_attempt(&pool, &hash, 3)
            .await
            .unwrap()
            .is_none(),
        "after the maximum attempts the code is dead"
    );
}

#[tokio::test]
async fn test_fr_con_001_concurrent_guesses_cannot_exceed_the_attempt_limit() {
    // covers: FR-CON-001, 10 T-41
    let pool = setup_db().await;
    let hash = digest();
    ConsoleRepository::issue_otp(&pool, &hash, &digest(), 600.0, 0.0)
        .await
        .unwrap();
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..30 {
        let pool = pool.clone();
        let hash = hash.clone();
        tasks.spawn(async move {
            ConsoleRepository::take_otp_attempt(&pool, &hash, 5)
                .await
                .unwrap()
                .is_some()
        });
    }
    let mut granted = 0;
    while let Some(result) = tasks.join_next().await {
        if result.unwrap() {
            granted += 1;
        }
    }
    assert_eq!(granted, 5, "30 parallel guesses, only 5 may be tried");
}

#[tokio::test]
async fn test_fr_con_001_an_expired_code_gives_no_attempt() {
    // covers: FR-CON-001
    let pool = setup_db().await;
    let hash = digest();
    ConsoleRepository::issue_otp(&pool, &hash, &digest(), 0.05, 0.0)
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(
        ConsoleRepository::take_otp_attempt(&pool, &hash, 5)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn test_fr_con_001_a_code_is_single_use_even_under_concurrency() {
    // covers: FR-CON-001
    let pool = setup_db().await;
    let hash = digest();
    ConsoleRepository::issue_otp(&pool, &hash, &digest(), 600.0, 0.0)
        .await
        .unwrap();
    let id = ConsoleRepository::take_otp_attempt(&pool, &hash, 5)
        .await
        .unwrap()
        .unwrap()
        .id;
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..12 {
        let pool = pool.clone();
        tasks.spawn(async move { ConsoleRepository::consume_otp(&pool, id).await.unwrap() });
    }
    let mut consumed = 0;
    while let Some(result) = tasks.join_next().await {
        if result.unwrap() {
            consumed += 1;
        }
    }
    assert_eq!(consumed, 1);
    // A consumed code leaves room for a new one right away.
    assert_eq!(
        ConsoleRepository::issue_otp(&pool, &hash, &digest(), 600.0, 3600.0)
            .await
            .unwrap(),
        OtpIssue::Issued
    );
}

#[tokio::test]
async fn test_fr_con_009_the_daily_email_budget_is_global_and_atomic() {
    // covers: FR-CON-009, 10 T-43
    let pool = setup_db().await;
    sqlx::query("DELETE FROM console_email_counters;")
        .execute(&pool)
        .await
        .unwrap();
    for _ in 0..3 {
        assert!(
            ConsoleRepository::take_email_budget(&pool, 3)
                .await
                .unwrap()
        );
    }
    assert!(
        !ConsoleRepository::take_email_budget(&pool, 3)
            .await
            .unwrap()
    );

    sqlx::query("DELETE FROM console_email_counters;")
        .execute(&pool)
        .await
        .unwrap();
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..40 {
        let pool = pool.clone();
        tasks.spawn(async move {
            ConsoleRepository::take_email_budget(&pool, 7)
                .await
                .unwrap()
        });
    }
    let mut granted = 0;
    while let Some(result) = tasks.join_next().await {
        if result.unwrap() {
            granted += 1;
        }
    }
    assert_eq!(granted, 7, "40 parallel requests, a budget of 7");
}

#[tokio::test]
async fn test_fr_con_001_an_account_needs_the_terms_and_is_created_once() {
    // covers: FR-CON-001
    let pool = setup_db().await;
    let address = email();
    assert!(
        ConsoleRepository::find_or_create_account(&pool, &address, &digest(), false)
            .await
            .unwrap()
            .is_none(),
        "no account is created without accepting the terms"
    );
    let (created, was_created) =
        ConsoleRepository::find_or_create_account(&pool, &address, &digest(), true)
            .await
            .unwrap()
            .unwrap();
    assert!(was_created);
    let (again, was_created_again) =
        ConsoleRepository::find_or_create_account(&pool, &address, &digest(), false)
            .await
            .unwrap()
            .unwrap();
    assert!(!was_created_again);
    assert_eq!(created.id, again.id);
}

#[tokio::test]
async fn test_fr_con_001_concurrent_sign_ups_create_one_account() {
    // covers: FR-CON-001
    let pool = setup_db().await;
    let address = email();
    let hash = digest();
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..10 {
        let (pool, address, hash) = (pool.clone(), address.clone(), hash.clone());
        tasks.spawn(async move {
            ConsoleRepository::find_or_create_account(&pool, &address, &hash, true)
                .await
                .unwrap()
                .unwrap()
        });
    }
    let (mut created, mut ids) = (0, std::collections::BTreeSet::new());
    while let Some(result) = tasks.join_next().await {
        let (account, was_created) = result.unwrap();
        created += i32::from(was_created);
        ids.insert(account.id.to_string());
    }
    assert_eq!(created, 1);
    assert_eq!(ids.len(), 1, "everybody sees the same account");
}

#[tokio::test]
async fn test_fr_con_010_sessions_expire_revoke_and_follow_the_account_status() {
    // covers: FR-CON-010
    let pool = setup_db().await;
    let acc = account(&pool).await;

    let live = digest();
    ConsoleRepository::create_session(&pool, acc.id, &live, 3600.0)
        .await
        .unwrap();
    assert_eq!(
        ConsoleRepository::find_active_session(&pool, &live)
            .await
            .unwrap()
            .map(|a| a.id),
        Some(acc.id)
    );

    let expired = digest();
    ConsoleRepository::create_session(&pool, acc.id, &expired, -1.0)
        .await
        .unwrap();
    assert!(
        ConsoleRepository::find_active_session(&pool, &expired)
            .await
            .unwrap()
            .is_none(),
        "an expired session is not valid"
    );

    assert!(
        ConsoleRepository::revoke_session(&pool, &live)
            .await
            .unwrap()
    );
    assert!(
        !ConsoleRepository::revoke_session(&pool, &live)
            .await
            .unwrap()
    );
    assert!(
        ConsoleRepository::find_active_session(&pool, &live)
            .await
            .unwrap()
            .is_none(),
        "a revoked session is not valid"
    );

    let other = digest();
    ConsoleRepository::create_session(&pool, acc.id, &other, 3600.0)
        .await
        .unwrap();
    ConsoleRepository::set_account_status(&pool, acc.id, AccountStatus::Disabled)
        .await
        .unwrap();
    assert!(
        ConsoleRepository::find_active_session(&pool, &other)
            .await
            .unwrap()
            .is_none(),
        "disabling the account invalidates its sessions at once"
    );
}

#[tokio::test]
async fn test_fr_con_010_the_database_never_holds_a_token_in_clear() {
    // covers: FR-CON-010
    let pool = setup_db().await;
    let acc = account(&pool).await;
    let token_hash = digest();
    ConsoleRepository::create_session(&pool, acc.id, &token_hash, 60.0)
        .await
        .unwrap();
    let stored: Vec<u8> =
        sqlx::query_scalar("SELECT token_sha256 FROM console_sessions WHERE account_id = $1;")
            .bind(acc.id.into_uuid())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(stored, token_hash);
    let columns: Vec<String> = sqlx::query_scalar(
        "SELECT column_name::text FROM information_schema.columns WHERE table_name IN ('console_sessions', 'console_otp_codes');",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    for forbidden in ["token", "code", "secret", "password"] {
        assert!(
            !columns.iter().any(|c| c == forbidden),
            "column named '{forbidden}' would suggest a clear-text secret"
        );
    }
}

#[tokio::test]
async fn test_fr_con_003_organizations_roles_and_the_owner_limit() {
    // covers: FR-CON-003, FR-CON-005
    let pool = setup_db().await;
    let owner = account(&pool).await;
    let stranger = account(&pool).await;

    let first = tenant("Org uno");
    assert!(
        ConsoleRepository::create_organization(&pool, owner.id, &first, 2)
            .await
            .unwrap()
    );
    let second = tenant("Org dos");
    assert!(
        ConsoleRepository::create_organization(&pool, owner.id, &second, 2)
            .await
            .unwrap()
    );
    let third = tenant("Org tres");
    assert!(
        !ConsoleRepository::create_organization(&pool, owner.id, &third, 2)
            .await
            .unwrap(),
        "the limit of organizations per account holds"
    );
    let missing: Option<i64> = sqlx::query_scalar("SELECT 1 FROM tenants WHERE id = $1;")
        .bind(third.id.into_uuid())
        .fetch_optional(&pool)
        .await
        .unwrap();
    assert!(
        missing.is_none(),
        "a refused organization leaves no tenant behind"
    );

    assert_eq!(
        ConsoleRepository::get_role(&pool, owner.id, first.id)
            .await
            .unwrap(),
        Some(ConsoleRole::Owner)
    );
    assert_eq!(
        ConsoleRepository::get_role(&pool, stranger.id, first.id)
            .await
            .unwrap(),
        None,
        "an organization of somebody else is indistinguishable from a missing one"
    );
    let listed = ConsoleRepository::list_organizations(&pool, owner.id)
        .await
        .unwrap();
    assert_eq!(
        listed.iter().map(|m| m.tenant.id).collect::<Vec<_>>(),
        vec![first.id, second.id]
    );
    assert!(
        ConsoleRepository::list_organizations(&pool, stranger.id)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn test_fr_con_005_concurrent_organizations_cannot_pass_the_limit() {
    // covers: FR-CON-005
    let pool = setup_db().await;
    let owner = account(&pool).await;
    let mut tasks = tokio::task::JoinSet::new();
    for n in 0..10 {
        let (pool, id) = (pool.clone(), owner.id);
        tasks.spawn(async move {
            ConsoleRepository::create_organization(&pool, id, &tenant(&format!("Org {n}")), 3)
                .await
                .unwrap()
        });
    }
    let mut created = 0;
    while let Some(result) = tasks.join_next().await {
        created += i32::from(result.unwrap());
    }
    assert_eq!(created, 3);
    assert_eq!(
        ConsoleRepository::count_owned_organizations(&pool, owner.id)
            .await
            .unwrap(),
        3
    );
}

#[tokio::test]
async fn test_deleting_an_account_removes_its_sessions_and_memberships() {
    // covers: FR-CON-010, docs/10 §7
    let pool = setup_db().await;
    let acc = account(&pool).await;
    let token = digest();
    ConsoleRepository::create_session(&pool, acc.id, &token, 3600.0)
        .await
        .unwrap();
    let org = tenant("Para borrar");
    ConsoleRepository::create_organization(&pool, acc.id, &org, 5)
        .await
        .unwrap();
    assert!(
        ConsoleRepository::delete_account(&pool, acc.id)
            .await
            .unwrap()
    );
    assert!(
        ConsoleRepository::find_active_session(&pool, &token)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        ConsoleRepository::get_role(&pool, acc.id, org.id)
            .await
            .unwrap(),
        None
    );
    assert!(
        !ConsoleRepository::delete_account(&pool, acc.id)
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn test_a_sole_owner_of_an_active_project_cannot_be_deleted_blindly() {
    // covers: docs/07 §9.13 (409 on deleteConsoleAccount)
    let pool = setup_db().await;
    let acc = account(&pool).await;
    let org = tenant("Con proyecto");
    ConsoleRepository::create_organization(&pool, acc.id, &org, 5)
        .await
        .unwrap();
    assert!(
        !ConsoleRepository::is_sole_owner_of_active_projects(&pool, acc.id)
            .await
            .unwrap(),
        "no project yet: nothing blocks the deletion"
    );
    sqlx::query(
        "INSERT INTO applications (id, tenant_id, name, status, allowed_device_scopes, allowed_origins) VALUES ($1, $2, 'p', 'ACTIVE', '{}', '{}');",
    )
    .bind(Uuid::now_v7())
    .bind(org.id.into_uuid())
    .execute(&pool)
    .await
    .unwrap();
    assert!(
        ConsoleRepository::is_sole_owner_of_active_projects(&pool, acc.id)
            .await
            .unwrap()
    );
}

#[test]
fn test_fr_con_011_the_account_debug_output_hides_the_email() {
    // covers: FR-CON-011, docs/10 §8.2
    let now = OffsetDateTime::now_utc();
    let acc = gs_store::ConsoleAccount {
        id: AccountId::new(),
        email: "secret.person@example.test".to_string(),
        status: AccountStatus::Active,
        terms_accepted_at: now,
        created_at: now,
        updated_at: now,
    };
    let rendered = format!("{acc:?}");
    assert!(!rendered.contains("secret.person"), "{rendered}");
    assert!(!rendered.contains("example.test"), "{rendered}");
}

#[test]
fn test_roles_form_a_hierarchy() {
    // covers: FR-CON-003, docs/06 §8.6
    assert!(ConsoleRole::Owner.at_least(ConsoleRole::Admin));
    assert!(ConsoleRole::Admin.at_least(ConsoleRole::Viewer));
    assert!(ConsoleRole::Viewer.at_least(ConsoleRole::Viewer));
    assert!(!ConsoleRole::Viewer.at_least(ConsoleRole::Admin));
    assert!(!ConsoleRole::Admin.at_least(ConsoleRole::Owner));
}
