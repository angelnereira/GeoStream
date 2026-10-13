//! Integration tests for the administrative use cases against PostgreSQL (T-F2-03, FR-ADM-001/002).
#![forbid(unsafe_code)]

use sqlx::{Column, Row};
use time::Duration;

use gs_domain::ids::{ApiKeyId, ApplicationId, TenantId};
use gs_service::admin::{
    AdminError, DEFAULT_APPLICATION_DEVICE_SCOPES, create_api_key, create_application,
    create_tenant, get_effective_policy, list_api_keys, revoke_api_key, revoke_api_key_with_bus,
    set_application_enabled, set_application_enabled_with_bus, set_policy_values,
    set_tenant_enabled, set_tenant_enabled_with_bus,
};
use gs_service::{Bus, InMemoryBus};
use gs_store::models::ApiKeyStatus;
use gs_store::{PgPool, connect_pool, run_migrations};

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

/// Reads every audit action recorded for a tenant, newest first.
async fn audit_actions(pool: &PgPool, tenant_id: TenantId) -> Vec<String> {
    let rows = sqlx::query(
        r#"
        SELECT action, actor_kind, target_kind, outcome
        FROM audit_events
        WHERE tenant_id = $1
        ORDER BY occurred_at DESC, id DESC;
        "#,
    )
    .bind(tenant_id.into_uuid())
    .fetch_all(pool)
    .await
    .expect("read audit events");

    rows.iter()
        .map(|row| {
            let action: String = row.get("action");
            let actor_kind: String = row.get("actor_kind");
            let outcome: String = row.get("outcome");
            assert_eq!(actor_kind, "OPERATOR", "operator actions must be audited");
            assert_eq!(outcome, "SUCCESS");
            action
        })
        .collect()
}

/// Returns the whole row of an api key as text to detect accidental plaintext secrets.
async fn api_key_row_dump(pool: &PgPool, key_id: ApiKeyId) -> String {
    let row = sqlx::query(
        r#"
        SELECT id::text AS id, tenant_id::text AS tenant_id, application_id::text AS application_id,
               encode(secret_sha256, 'hex') AS secret_sha256, scopes::text AS scopes,
               COALESCE(label, '') AS label, status, expires_at::text AS expires_at,
               last_used_at::text AS last_used_at, created_at::text AS created_at,
               COALESCE(revoked_at::text, '') AS revoked_at
        FROM api_keys
        WHERE id = $1;
        "#,
    )
    .bind(key_id.into_uuid())
    .fetch_one(pool)
    .await
    .expect("read api key row");

    row.columns()
        .iter()
        .map(|column| {
            let value: String = row
                .try_get(column.name())
                .unwrap_or_else(|_| "<null>".to_string());
            format!("{}={value}", column.name())
        })
        .collect::<Vec<_>>()
        .join(";")
}

#[tokio::test]
async fn test_tenant_and_application_creation_are_audited() {
    // covers: FR-ADM-001, docs/10 §8.3
    let pool = setup_db().await;

    let tenant = create_tenant(&pool, "Service Tenant")
        .await
        .expect("create tenant");
    let application = create_application(
        &pool,
        &tenant.id.to_string(),
        "service-app",
        None,
        Some(vec!["https://example.com".to_string()]),
    )
    .await
    .expect("create application");

    // Default device scopes come from docs/11 §2.
    assert_eq!(
        application.allowed_device_scopes,
        DEFAULT_APPLICATION_DEVICE_SCOPES
            .iter()
            .map(|s| (*s).to_string())
            .collect::<Vec<_>>()
    );
    assert_eq!(application.allowed_origins, vec!["https://example.com"]);

    let actions = audit_actions(&pool, tenant.id).await;
    assert!(actions.contains(&"tenant.created".to_string()));
    assert!(actions.contains(&"application.created".to_string()));
}

#[tokio::test]
async fn test_tenant_and_application_can_be_disabled_and_enabled() {
    // covers: FR-ADM-001, docs/10 §8.3
    let pool = setup_db().await;
    let tenant = create_tenant(&pool, "Toggle Tenant").await.expect("tenant");
    let application = create_application(&pool, &tenant.id.to_string(), "toggle-app", None, None)
        .await
        .expect("application");

    let disabled = set_tenant_enabled(&pool, &tenant.id.to_string(), false)
        .await
        .expect("disable tenant");
    assert_eq!(disabled.status, gs_store::models::TenantStatus::Disabled);

    let disabled_app = set_application_enabled(&pool, &application.id.to_string(), false)
        .await
        .expect("disable application");
    assert_eq!(
        disabled_app.status,
        gs_store::models::ApplicationStatus::Disabled
    );

    let enabled = set_application_enabled(&pool, &application.id.to_string(), true)
        .await
        .expect("enable application");
    assert_eq!(enabled.status, gs_store::models::ApplicationStatus::Active);

    let actions = audit_actions(&pool, tenant.id).await;
    assert!(actions.contains(&"tenant.disabled".to_string()));
    assert!(actions.contains(&"application.disabled".to_string()));
    assert!(actions.contains(&"application.enabled".to_string()));
}

#[tokio::test]
async fn test_disable_without_state_change_is_a_no_op() {
    // covers: FR-ADM-001
    let pool = setup_db().await;
    let tenant = create_tenant(&pool, "Idempotent Tenant")
        .await
        .expect("tenant");

    let disabled = set_tenant_enabled(&pool, &tenant.id.to_string(), false)
        .await
        .expect("disable");
    let again = set_tenant_enabled(&pool, &tenant.id.to_string(), false)
        .await
        .expect("second disable is a no-op");
    assert_eq!(disabled.id, again.id);

    let actions = audit_actions(&pool, tenant.id).await;
    assert_eq!(
        actions
            .iter()
            .filter(|action| action.as_str() == "tenant.disabled")
            .count(),
        1,
        "a repeated disable must not be audited again"
    );
}

#[tokio::test]
async fn test_api_key_creation_shows_credential_once_and_audits_it() {
    // covers: FR-ADM-001, FR-AUTH-001, docs/06 §3.1–§3.2
    let pool = setup_db().await;
    let tenant = create_tenant(&pool, "Key Tenant").await.expect("tenant");
    let application = create_application(&pool, &tenant.id.to_string(), "key-app", None, None)
        .await
        .expect("application");

    let created = create_api_key(
        &pool,
        &application.id.to_string(),
        Some(vec!["tokens:mint".to_string()]),
        Some("ci"),
        Some(Duration::days(90)),
    )
    .await
    .expect("create api key");

    let credential = created.credential.as_str();
    assert!(
        credential.starts_with(&format!("gsk_{}.", created.key.id.into_uuid().simple())),
        "unexpected credential shape"
    );

    let keys = list_api_keys(&pool, &application.id.to_string())
        .await
        .expect("list keys");
    assert_eq!(keys.len(), 1);
    assert_eq!(keys[0].id, created.key.id);
    assert_eq!(keys[0].status, ApiKeyStatus::Active);
    assert_eq!(keys[0].label.as_deref(), Some("ci"));

    // The database never stores the secret: only 32 raw bytes of SHA-256.
    let row_dump = api_key_row_dump(&pool, created.key.id).await;
    let secret = credential
        .rsplit('.')
        .next()
        .expect("credential must carry a secret");
    assert!(
        !row_dump.contains(secret),
        "the plaintext secret must never be persisted: {row_dump}"
    );
    assert!(
        row_dump.contains("secret_sha256="),
        "the hash must be stored: {row_dump}"
    );

    let actions = audit_actions(&pool, tenant.id).await;
    assert!(actions.contains(&"api_key.created".to_string()));

    // Revocation is terminal and audited.
    let revoked = revoke_api_key(&pool, &created.key.id.to_string())
        .await
        .expect("revoke");
    assert_eq!(revoked.status, ApiKeyStatus::Revoked);
    let again = revoke_api_key(&pool, &created.key.id.to_string())
        .await
        .expect("second revoke keeps REVOKED");
    assert_eq!(again.status, ApiKeyStatus::Revoked);

    let actions = audit_actions(&pool, tenant.id).await;
    assert_eq!(
        actions
            .iter()
            .filter(|action| action.as_str() == "api_key.revoked")
            .count(),
        1
    );
}

#[tokio::test]
async fn test_api_key_default_scopes_exclude_locations_read() {
    // covers: FR-AUTH-001, docs/06 §3.4
    let pool = setup_db().await;
    let tenant = create_tenant(&pool, "Scope Tenant").await.expect("tenant");
    let application = create_application(&pool, &tenant.id.to_string(), "scope-app", None, None)
        .await
        .expect("application");

    let created = create_api_key(&pool, &application.id.to_string(), None, None, None)
        .await
        .expect("create api key with default scopes");

    assert!(
        !created.key.scopes.iter().any(|s| s == "locations:read"),
        "locations:read must not be granted by default"
    );
    assert_eq!(created.key.scopes.len(), 9);

    assert!(matches!(
        create_api_key(
            &pool,
            &application.id.to_string(),
            Some(vec!["not-a-scope".to_string()]),
            None,
            None,
        )
        .await,
        Err(AdminError::InvalidScopes(_))
    ));

    assert!(matches!(
        create_application(
            &pool,
            &tenant.id.to_string(),
            "bad-scopes",
            Some(vec!["not-a-device-scope".to_string()]),
            None,
        )
        .await,
        Err(AdminError::InvalidScopes(_))
    ));
}

#[tokio::test]
async fn test_unknown_entities_and_invalid_ids_are_rejected() {
    // covers: FR-ADM-001, FR-API-006
    let pool = setup_db().await;

    assert!(matches!(
        set_tenant_enabled(&pool, "not-a-tenant-id", false).await,
        Err(AdminError::InvalidTenantId { .. })
    ));
    assert!(matches!(
        set_application_enabled(&pool, "not-an-app-id", false).await,
        Err(AdminError::InvalidApplicationId { .. })
    ));
    assert!(matches!(
        revoke_api_key(&pool, "not-a-key-id").await,
        Err(AdminError::InvalidApiKeyId { .. })
    ));

    // Well-formed but unknown identifiers are "not found", never "unauthorized".
    let missing_tenant = TenantId::new().to_string();
    assert!(matches!(
        get_effective_policy(&pool, &missing_tenant).await,
        Err(AdminError::TenantNotFound { .. })
    ));
    let missing_app = ApplicationId::new().to_string();
    assert!(matches!(
        list_api_keys(&pool, &missing_app).await,
        Err(AdminError::ApplicationNotFound { .. })
    ));
    let missing_key = ApiKeyId::new().to_string();
    assert!(matches!(
        revoke_api_key(&pool, &missing_key).await,
        Err(AdminError::ApiKeyNotFound { .. })
    ));
}

#[tokio::test]
async fn test_effective_policy_falls_back_to_defaults_and_validates_ranges() {
    // covers: FR-ADM-002, docs/17 §4
    let pool = setup_db().await;
    let tenant = create_tenant(&pool, "Policy Tenant").await.expect("tenant");
    let tenant_ref = tenant.id.to_string();

    // Absent keys take the default of docs/17 §4.
    let defaults = get_effective_policy(&pool, &tenant_ref)
        .await
        .expect("effective policy");
    assert_eq!(defaults.max_devices_per_subject, 10);
    assert!(!defaults.history_allowed);

    // A valid override is stored and only the override is persisted.
    let updated = set_policy_values(
        &pool,
        &tenant_ref,
        &["max_devices_per_subject=4".to_string()],
    )
    .await
    .expect("set policy");
    assert_eq!(updated.max_devices_per_subject, 4);
    assert_eq!(updated.default_session_ttl_seconds, 86400);

    let again = get_effective_policy(&pool, &tenant_ref).await.expect("get");
    assert_eq!(again.max_devices_per_subject, 4);
    assert_eq!(again.default_session_ttl_seconds, 86400);

    // Out-of-range values fail with an explicit message and change nothing.
    assert!(matches!(
        set_policy_values(
            &pool,
            &tenant_ref,
            &["max_devices_per_subject=0".to_string()]
        )
        .await,
        Err(AdminError::InvalidPolicy(_))
    ));
    assert!(matches!(
        set_policy_values(&pool, &tenant_ref, &["not_a_key=1".to_string()]).await,
        Err(AdminError::InvalidPolicy(_))
    ));
    assert!(matches!(
        set_policy_values(&pool, &tenant_ref, &["history_allowed".to_string()]).await,
        Err(AdminError::InvalidPolicy(_))
    ));

    // Cross-validation uses the effective policy: default TTL above a lowered max is invalid.
    assert!(matches!(
        set_policy_values(
            &pool,
            &tenant_ref,
            &[
                "max_session_ttl_seconds=3600".to_string(),
                "default_session_ttl_seconds=7200".to_string(),
            ],
        )
        .await,
        Err(AdminError::InvalidPolicy(_))
    ));

    let unchanged = get_effective_policy(&pool, &tenant_ref).await.expect("get");
    assert_eq!(
        unchanged.max_devices_per_subject, 4,
        "rejected assignments must not modify the stored policy"
    );

    // Only the override is stored: the previous override survives an unrelated assignment.
    let combined = set_policy_values(&pool, &tenant_ref, &["publish_burst=9".to_string()])
        .await
        .expect("set second override");
    assert_eq!(combined.max_devices_per_subject, 4);
    assert_eq!(combined.publish_burst, 9);
}

#[tokio::test]
async fn test_signing_key_generation_is_delegated_without_a_database() {
    // covers: FR-ADM-001, FR-AUTH-004, docs/06 §4.4
    let pool = setup_db().await;
    let tenant = create_tenant(&pool, "Keys Tenant").await.expect("tenant");
    let _ = pool; // the use case needs no database, but keeps the setup symmetric
    let kid = format!("test-{}", &tenant.id.to_string()[4..12]);
    let dir = std::env::temp_dir().join(format!("gs-service-signing-{}", tenant.id));
    let _ = std::fs::remove_dir_all(&dir);

    let generated = gs_service::admin::generate_signing_key_pair(&kid, &dir)
        .expect("generate signing key pair");
    assert!(generated.private_key_path.exists());
    assert!(generated.public_key_path.exists());

    let second = gs_service::admin::generate_signing_key_pair(&kid, &dir);
    assert!(
        second.is_err(),
        "existing signing keys must never be overwritten"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn test_tenant_and_application_disable_enable_with_bus_invalidation() {
    // covers: FR-ADM-003, docs/16 §3, docs/04 §6.1, docs/06 §7
    let pool = setup_db().await;
    let tenant = create_tenant(&pool, "Bus Invalidation Tenant")
        .await
        .expect("tenant");
    let application = create_application(&pool, &tenant.id.to_string(), "bus-app", None, None)
        .await
        .expect("application");
    let created_key = create_api_key(&pool, &application.id.to_string(), None, None, None)
        .await
        .expect("api key");

    let bus = InMemoryBus::new();
    let sub_id = bus.subscribe("gs:bus:global").await.expect("subscribe");

    // 1. Disable tenant with bus -> TenantDisabled published
    let disabled_tenant = set_tenant_enabled_with_bus(
        &pool,
        &tenant.id.to_string(),
        false,
        Some(&*bus),
        Some("gs"),
    )
    .await
    .expect("disable tenant with bus");
    assert_eq!(
        disabled_tenant.status,
        gs_store::models::TenantStatus::Disabled
    );

    let msg = bus
        .next_message(sub_id)
        .await
        .expect("msg")
        .expect("some msg");
    assert_eq!(msg.topic, "gs:bus:global");
    let payload: serde_json::Value = serde_json::from_slice(&msg.payload).expect("json payload");
    assert_eq!(payload["event"], "TenantDisabled");
    assert_eq!(payload["tenant_id"], tenant.id.to_string());
    assert!(payload["at"].is_string());

    // Disabling again (no-op) must not publish a duplicate message
    let again = set_tenant_enabled_with_bus(
        &pool,
        &tenant.id.to_string(),
        false,
        Some(&*bus),
        Some("gs"),
    )
    .await
    .expect("disable again");
    assert_eq!(again.status, gs_store::models::TenantStatus::Disabled);

    // 2. Enable tenant with bus -> TenantEnabled published
    let enabled_tenant =
        set_tenant_enabled_with_bus(&pool, &tenant.id.to_string(), true, Some(&*bus), Some("gs"))
            .await
            .expect("enable tenant with bus");
    assert_eq!(
        enabled_tenant.status,
        gs_store::models::TenantStatus::Active
    );

    let msg = bus
        .next_message(sub_id)
        .await
        .expect("msg")
        .expect("some msg");
    assert_eq!(msg.topic, "gs:bus:global");
    let payload: serde_json::Value = serde_json::from_slice(&msg.payload).expect("json payload");
    assert_eq!(payload["event"], "TenantEnabled");
    assert_eq!(payload["tenant_id"], tenant.id.to_string());

    // 3. Disable application with bus -> ApplicationDisabled published
    let disabled_app = set_application_enabled_with_bus(
        &pool,
        &application.id.to_string(),
        false,
        Some(&*bus),
        Some("gs"),
    )
    .await
    .expect("disable application with bus");
    assert_eq!(
        disabled_app.status,
        gs_store::models::ApplicationStatus::Disabled
    );

    let msg = bus
        .next_message(sub_id)
        .await
        .expect("msg")
        .expect("some msg");
    assert_eq!(msg.topic, "gs:bus:global");
    let payload: serde_json::Value = serde_json::from_slice(&msg.payload).expect("json payload");
    assert_eq!(payload["event"], "ApplicationDisabled");
    assert_eq!(payload["tenant_id"], tenant.id.to_string());
    assert_eq!(payload["application_id"], application.id.to_string());

    // 4. Enable application with bus -> ApplicationEnabled published
    let enabled_app = set_application_enabled_with_bus(
        &pool,
        &application.id.to_string(),
        true,
        Some(&*bus),
        Some("gs"),
    )
    .await
    .expect("enable application with bus");
    assert_eq!(
        enabled_app.status,
        gs_store::models::ApplicationStatus::Active
    );

    let msg = bus
        .next_message(sub_id)
        .await
        .expect("msg")
        .expect("some msg");
    assert_eq!(msg.topic, "gs:bus:global");
    let payload: serde_json::Value = serde_json::from_slice(&msg.payload).expect("json payload");
    assert_eq!(payload["event"], "ApplicationEnabled");
    assert_eq!(payload["tenant_id"], tenant.id.to_string());
    assert_eq!(payload["application_id"], application.id.to_string());

    // 5. Revoke API key with bus -> ApiKeyRevoked published
    let revoked_key = revoke_api_key_with_bus(
        &pool,
        &created_key.key.id.to_string(),
        Some(&*bus),
        Some("gs"),
    )
    .await
    .expect("revoke key with bus");
    assert_eq!(revoked_key.status, ApiKeyStatus::Revoked);

    let msg = bus
        .next_message(sub_id)
        .await
        .expect("msg")
        .expect("some msg");
    assert_eq!(msg.topic, "gs:bus:global");
    let payload: serde_json::Value = serde_json::from_slice(&msg.payload).expect("json payload");
    assert_eq!(payload["event"], "ApiKeyRevoked");
    assert_eq!(payload["tenant_id"], tenant.id.to_string());
    assert_eq!(payload["api_key_id"], created_key.key.id.to_string());
}
