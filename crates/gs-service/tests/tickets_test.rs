//! Stream tickets: issuance and atomic redemption against PostgreSQL (T-F4-01, FR-AUTH-006,
//! docs/06 §5, docs/08 §3).
//!
//! Acceptance of FR-AUTH-006: a second use of the same ticket, or a use after its expiry,
//! is rejected (HTTP 401 `UNAUTHENTICATED` at the upgrade, i.e. `TicketError::Unauthenticated`
//! here).
#![forbid(unsafe_code)]

use std::time::Duration;

use gs_auth::jwt::SigningKeySet;
use gs_auth::{generate_signing_key, parse_private_pem};
use gs_domain::error::{AsErrorCode, ErrorCode};
use gs_service::admin::{create_api_key, create_application, create_tenant};
use gs_service::auth::{
    ApplicationPrincipal, VerifiedPrincipal, verify_application_principal,
    verify_device_token_principal,
};
use gs_service::tickets::{
    IssuedTicket, TicketCaller, TicketConfig, TicketError, TicketGrant, issue_stream_ticket,
    redeem_stream_ticket, ticket_key, verify_ticket_principal_state,
};
use gs_service::tokens::{TokenRequest, TokenTtlBounds, mint_device_token};
use gs_service::{EphemeralStore, InMemoryEphemeralStore};
use gs_store::{PgPool as StorePool, connect_pool, run_migrations};
use time::OffsetDateTime;

const ISSUER: &str = "https://geostream.example.com";
const INSTALLATION: &str = "0192b6c0-a1e4-4c3f-9d2a-7b5e8f1c0d3a";

fn test_database_url() -> String {
    std::env::var("GS_DATABASE_URL").unwrap_or_else(|_| {
        "postgres://geostream:geostream_dev_password@127.0.0.1:5432/geostream_test".to_string()
    })
}

/// Tenant, application and the credentials of an application key and a device token.
struct World {
    pool: StorePool,
    tenant_id: String,
    application_id: String,
    key_credential: String,
    token: String,
    keys: SigningKeySet,
}

async fn setup(suffix: &str, device_scopes: &[&str]) -> World {
    let pool = connect_pool(&test_database_url()).await.expect("connect");
    run_migrations(&pool).await.expect("migrate");

    let tenant = create_tenant(&pool, &format!("Ticket Tenant {suffix}"))
        .await
        .expect("tenant");
    let application = create_application(
        &pool,
        &tenant.id.to_string(),
        &format!("ticket-app-{suffix}"),
        Some(vec![
            "sessions:create".to_string(),
            "stream:connect".to_string(),
        ]),
        None,
    )
    .await
    .expect("application");
    let key = create_api_key(
        &pool,
        &application.id.to_string(),
        Some(vec![
            "tokens:mint".to_string(),
            "stream:connect".to_string(),
        ]),
        Some("ticket"),
        None,
    )
    .await
    .expect("api key");

    let kid = format!("ticket-{suffix}");
    let generated = generate_signing_key(&kid).expect("generate key");
    let signing = parse_private_pem(generated.private_pem()).expect("parse key");
    let mut keys = SigningKeySet::default();
    keys.insert_signing_key(&kid, signing.clone());

    let scopes: Vec<String> = device_scopes.iter().map(|s| (*s).to_string()).collect();
    let minted = mint_device_token(
        &pool,
        key.credential.as_str(),
        &TokenRequest {
            external_subject_id: format!("ticket-user-{suffix}"),
            installation_id: INSTALLATION.to_string(),
            platform: "ANDROID".to_string(),
            app_version: None,
            sdk_version: None,
            scopes: Some(scopes),
            ttl_seconds: Some(900),
        },
        TokenTtlBounds {
            min_seconds: 300,
            default_seconds: 900,
            max_seconds: 3600,
        },
        ISSUER,
        &signing,
        &kid,
    )
    .await
    .expect("mint token");

    World {
        pool,
        tenant_id: tenant.id.to_string(),
        application_id: application.id.to_string(),
        key_credential: key.credential.to_string(),
        token: minted.response.access_token.clone(),
        keys,
    }
}

async fn application_caller(world: &World) -> ApplicationPrincipal {
    verify_application_principal(&world.pool, &world.key_credential)
        .await
        .expect("application principal")
}

async fn device_caller(world: &World) -> VerifiedPrincipal {
    verify_device_token_principal(&world.pool, &world.token, &world.keys, ISSUER, 30)
        .await
        .expect("device principal")
}

fn config() -> TicketConfig {
    TicketConfig {
        ttl: Duration::from_secs(30),
        app_max_connection_age: Duration::from_secs(3600),
    }
}

#[tokio::test]
async fn test_fr_auth_006_application_ticket_is_single_use() {
    // covers: FR-AUTH-006, docs/06 §5
    let world = setup("app-single", &["stream:connect"]).await;
    let store = InMemoryEphemeralStore::new();
    let caller = application_caller(&world).await;

    let issued: IssuedTicket = issue_stream_ticket(
        store.as_ref(),
        "gs:test",
        &TicketCaller::Application(&caller),
        &config(),
        OffsetDateTime::now_utc(),
    )
    .await
    .expect("issue ticket");

    // The grant is bound to the application principal (docs/06 §5).
    assert_eq!(
        issued.grant.principal_kind,
        gs_service::tickets::TicketPrincipalKind::Application
    );
    assert_eq!(issued.grant.tenant_id.to_string(), world.tenant_id);
    assert_eq!(
        issued.grant.application_id.to_string(),
        world.application_id
    );
    assert_eq!(issued.grant.subject_id, None);
    assert_eq!(issued.grant.device_id, None);
    assert_eq!(issued.grant.api_key_id, Some(caller.api_key.id));
    assert_eq!(issued.grant.scopes, caller.api_key.scopes);
    assert!(issued.ticket.as_str().starts_with("gst_"));

    // The ticket expiry is `now + GS_TICKET_TTL_S`.
    let expires_at = OffsetDateTime::parse(
        &issued.grant.auth_expires_at,
        &time::format_description::well_known::Rfc3339,
    )
    .expect("parse auth_expires_at");
    assert!(expires_at > OffsetDateTime::now_utc());

    // First redemption succeeds and deletes the entry.
    let grant = redeem_stream_ticket(
        store.as_ref(),
        "gs:test",
        issued.ticket.as_str(),
        OffsetDateTime::now_utc(),
    )
    .await
    .expect("redeem ticket");
    assert_eq!(grant, issued.grant);
    verify_ticket_principal_state(&world.pool, &grant)
        .await
        .expect("principal state must be active");

    // A second use of the same ticket is rejected: 401 without upgrade.
    let second = redeem_stream_ticket(
        store.as_ref(),
        "gs:test",
        issued.ticket.as_str(),
        OffsetDateTime::now_utc(),
    )
    .await
    .expect_err("a used ticket must not be redeemable");
    assert!(matches!(second, TicketError::Unauthenticated));
    assert_eq!(second.error_code(), ErrorCode::Unauthenticated);
    assert_eq!(ErrorCode::Unauthenticated.http_status(), Some(401));
}

#[tokio::test]
async fn test_fr_auth_006_device_ticket_is_single_use_and_carries_the_token_expiry() {
    // covers: FR-AUTH-006, docs/06 §5
    let world = setup("device-single", &["stream:connect", "sessions:create"]).await;
    let store = InMemoryEphemeralStore::new();
    let caller = device_caller(&world).await;

    let issued = issue_stream_ticket(
        store.as_ref(),
        "gs:test",
        &TicketCaller::Device(&caller),
        &config(),
        OffsetDateTime::now_utc(),
    )
    .await
    .expect("issue ticket");

    assert_eq!(
        issued.grant.principal_kind,
        gs_service::tickets::TicketPrincipalKind::Device
    );
    assert_eq!(issued.grant.subject_id, Some(caller.subject.id));
    assert_eq!(issued.grant.device_id, Some(caller.device.id));
    assert_eq!(issued.grant.api_key_id, None);
    assert_eq!(issued.grant.scopes, caller.claims.scp);

    // `auth_expires_at` is the `exp` of the token that requested the ticket (docs/06 §5).
    let auth_expires_at = OffsetDateTime::parse(
        &issued.grant.auth_expires_at,
        &time::format_description::well_known::Rfc3339,
    )
    .expect("parse auth_expires_at");
    let expected = OffsetDateTime::from_unix_timestamp(caller.claims.exp).expect("exp");
    assert_eq!(auth_expires_at.unix_timestamp(), expected.unix_timestamp());

    let grant = redeem_stream_ticket(
        store.as_ref(),
        "gs:test",
        issued.ticket.as_str(),
        OffsetDateTime::now_utc(),
    )
    .await
    .expect("redeem ticket");
    verify_ticket_principal_state(&world.pool, &grant)
        .await
        .expect("principal state must be active");

    assert!(matches!(
        redeem_stream_ticket(
            store.as_ref(),
            "gs:test",
            issued.ticket.as_str(),
            OffsetDateTime::now_utc()
        )
        .await,
        Err(TicketError::Unauthenticated)
    ));
}

#[tokio::test]
async fn test_fr_auth_006_ticket_is_rejected_after_its_ttl() {
    // covers: FR-AUTH-006, docs/06 §5, docs/17 §2.4 (GS_TICKET_TTL_S)
    let world = setup("ttl", &["stream:connect"]).await;
    let store = InMemoryEphemeralStore::new();
    let caller = application_caller(&world).await;
    let now = OffsetDateTime::now_utc();

    let issued = issue_stream_ticket(
        store.as_ref(),
        "gs:test",
        &TicketCaller::Application(&caller),
        &TicketConfig {
            ttl: Duration::from_secs(5),
            app_max_connection_age: Duration::from_secs(3600),
        },
        now,
    )
    .await
    .expect("issue ticket");

    // Still inside the TTL the ticket redeems.
    assert!(
        redeem_stream_ticket(store.as_ref(), "gs:test", issued.ticket.as_str(), now)
            .await
            .is_ok()
    );

    // A new ticket, once its TTL has elapsed, is expired (single use, and never reusable).
    let second = issue_stream_ticket(
        store.as_ref(),
        "gs:test",
        &TicketCaller::Application(&caller),
        &TicketConfig {
            ttl: Duration::from_secs(5),
            app_max_connection_age: Duration::from_secs(3600),
        },
        now,
    )
    .await
    .expect("issue second ticket");
    // The in-memory store expires entries against the wall clock, so the wait is real.
    tokio::time::sleep(Duration::from_secs(6)).await;
    assert!(matches!(
        redeem_stream_ticket(
            store.as_ref(),
            "gs:test",
            second.ticket.as_str(),
            OffsetDateTime::now_utc()
        )
        .await,
        Err(TicketError::Unauthenticated)
    ));
}

#[tokio::test]
async fn test_malformed_and_unknown_tickets_are_rejected() {
    // covers: FR-AUTH-006, docs/08 §3
    let store = InMemoryEphemeralStore::new();
    let now = OffsetDateTime::now_utc();
    for candidate in ["", "gst_", "not-a-ticket", "gst_too_short"] {
        assert!(matches!(
            redeem_stream_ticket(store.as_ref(), "gs:test", candidate, now).await,
            Err(TicketError::Unauthenticated)
        ));
    }

    // A well formed ticket that was never issued is unknown.
    let unknown = gs_auth::ticket::generate_ticket();
    assert!(matches!(
        redeem_stream_ticket(store.as_ref(), "gs:test", &unknown, now).await,
        Err(TicketError::Unauthenticated)
    ));
}

#[tokio::test]
async fn test_expired_auth_window_is_rejected_at_redemption() {
    // covers: FR-AUTH-006, docs/06 §5 (`auth_expires_at`)
    let world = setup("auth-window", &["stream:connect"]).await;
    let store = InMemoryEphemeralStore::new();
    let caller = application_caller(&world).await;

    let issued = issue_stream_ticket(
        store.as_ref(),
        "gs:test",
        &TicketCaller::Application(&caller),
        &config(),
        OffsetDateTime::now_utc(),
    )
    .await
    .expect("issue ticket");

    // A grant whose authentication window already closed is not redeemable.
    let mut stale = issued.grant.clone();
    stale.auth_expires_at = "2020-01-01T00:00:00.000Z".to_string();
    store
        .set_ticket(
            &ticket_key("gs:test", issued.ticket.as_str()),
            &serde_json::to_vec(&stale).expect("serialize grant"),
            Duration::from_secs(30),
        )
        .await
        .expect("store stale grant");

    assert!(matches!(
        redeem_stream_ticket(
            store.as_ref(),
            "gs:test",
            issued.ticket.as_str(),
            OffsetDateTime::now_utc()
        )
        .await,
        Err(TicketError::Unauthenticated)
    ));
}

#[tokio::test]
async fn test_disabled_tenant_or_application_blocks_redemption() {
    // covers: FR-AUTH-006, docs/08 §3 (403 ACCOUNT_DISABLED)
    let world = setup("disabled", &["stream:connect"]).await;
    let store = InMemoryEphemeralStore::new();
    let caller = application_caller(&world).await;

    let issued = issue_stream_ticket(
        store.as_ref(),
        "gs:test",
        &TicketCaller::Application(&caller),
        &config(),
        OffsetDateTime::now_utc(),
    )
    .await
    .expect("issue ticket");

    // Disabling the application after the ticket was issued blocks the upgrade.
    gs_service::admin::set_application_enabled(&world.pool, &world.application_id, false)
        .await
        .expect("disable application");
    let grant = redeem_stream_ticket(
        store.as_ref(),
        "gs:test",
        issued.ticket.as_str(),
        OffsetDateTime::now_utc(),
    )
    .await
    .expect("the ticket itself is still redeemable");
    let error = verify_ticket_principal_state(&world.pool, &grant)
        .await
        .expect_err("disabled application must block the upgrade");
    assert!(matches!(error, TicketError::AccountDisabled(_)));
    assert_eq!(error.error_code(), ErrorCode::AccountDisabled);
    assert_eq!(ErrorCode::AccountDisabled.http_status(), Some(403));

    // Re-enabling restores the upgrade.
    gs_service::admin::set_application_enabled(&world.pool, &world.application_id, true)
        .await
        .expect("enable application");
    verify_ticket_principal_state(&world.pool, &grant)
        .await
        .expect("enabled application must pass");
}

#[tokio::test]
async fn test_ticket_key_uses_the_digest_and_never_the_ticket() {
    // covers: docs/06 §5, docs/11 §3
    let ticket = gs_auth::ticket::generate_ticket();
    let key = ticket_key("gs", &ticket);
    assert!(key.starts_with("gs:ticket:"), "key: {key}");
    assert!(
        !key.contains(&ticket),
        "the ticket must not appear in the key"
    );
    let digest = gs_auth::ticket::hash_ticket_hex(&ticket);
    assert_eq!(key, format!("gs:ticket:{digest}"));
}

#[tokio::test]
async fn test_ticket_errors_map_to_the_documented_codes() {
    // covers: docs/09 §6, docs/07 §5
    assert_eq!(
        TicketError::Unauthenticated.error_code(),
        ErrorCode::Unauthenticated
    );
    assert_eq!(
        TicketError::AccountDisabled("application disabled".to_string()).error_code(),
        ErrorCode::AccountDisabled
    );
    assert_eq!(
        TicketError::TemporarilyUnavailable.error_code(),
        ErrorCode::TemporarilyUnavailable
    );
    // A malformed grant or timestamp is a server side fault, never an authentication one.
    assert_eq!(
        TicketError::Internal("grant".to_string()).error_code(),
        ErrorCode::Internal
    );
}

#[tokio::test]
async fn test_grant_roundtrip_through_json() {
    // covers: docs/06 §5, docs/11 §3 (JSON compacto)
    let world = setup("roundtrip", &["stream:connect"]).await;
    let caller = application_caller(&world).await;
    let store = InMemoryEphemeralStore::new();
    let issued = issue_stream_ticket(
        store.as_ref(),
        "gs:test",
        &TicketCaller::Application(&caller),
        &config(),
        OffsetDateTime::now_utc(),
    )
    .await
    .expect("issue ticket");

    let bytes = serde_json::to_vec(&issued.grant).expect("serialize");
    let json: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(json["principal_kind"], serde_json::json!("application"));
    assert_eq!(json["tenant_id"], serde_json::json!(world.tenant_id));
    assert!(json["subject_id"].is_null(), "absent ids must be omitted");
    let restored: TicketGrant = serde_json::from_slice(&bytes).expect("deserialize");
    assert_eq!(restored, issued.grant);
}
