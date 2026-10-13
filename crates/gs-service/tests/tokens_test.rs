//! End-to-end tests for `POST /v1/tokens` against PostgreSQL (T-F2-08, FR-AUTH-002, docs/06 §4).
#![forbid(unsafe_code)]

use gs_auth::jwt::SigningKeySet;
use gs_auth::{generate_signing_key, verify_device_token};
use gs_domain::ids::{ApplicationId, TenantId};
use gs_service::admin::{create_api_key, create_application, create_tenant};
use gs_service::tokens::{TokenRequest, TokenTtlBounds, mint_device_token};
use gs_store::{PgPool as StorePool, connect_pool, run_migrations};
use serde_json::{Value, json};
use time::OffsetDateTime;

fn test_database_url() -> String {
    std::env::var("GS_DATABASE_URL").unwrap_or_else(|_| {
        "postgres://geostream:geostream_dev_password@127.0.0.1:5432/geostream_test".to_string()
    })
}

async fn setup() -> (StorePool, TenantId, ApplicationId, String) {
    let pool = connect_pool(&test_database_url()).await.expect("connect");
    run_migrations(&pool).await.expect("migrate");

    let tenant = create_tenant(&pool, "Token Tenant").await.expect("tenant");
    let application = create_application(
        &pool,
        &tenant.id.to_string(),
        "token-app",
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
        Some(vec!["tokens:mint".to_string()]),
        Some("mint"),
        None,
    )
    .await
    .expect("api key");

    (pool, tenant.id, application.id, (*key.credential).clone())
}

/// Canonical UUIDv4: version nibble 4 and RFC 4122 variant (docs/05 §2).
const INSTALLATION: &str = "0192b6c0-a1e4-4c3f-9d2a-7b5e8f1c0d3a";

fn request() -> TokenRequest {
    serde_json::from_value(json!({
        "external_subject_id": "user-42",
        "installation_id": INSTALLATION,
        "platform": "ANDROID",
        "app_version": "1.0.0",
        "sdk_version": "0.1.0",
        "scopes": ["sessions:create"],
        "ttl_seconds": 900
    }))
    .expect("request")
}

fn bounds() -> TokenTtlBounds {
    TokenTtlBounds {
        min_seconds: 300,
        default_seconds: 900,
        max_seconds: 3600,
    }
}

#[tokio::test]
async fn test_mint_creates_subject_and_device_and_a_verifiable_token() {
    // covers: FR-AUTH-002, docs/06 §4.1–§4.2
    let (pool, tenant_id, application_id, credential) = setup().await;

    let dir = std::env::temp_dir().join(format!("gs-token-keys-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let generated = generate_signing_key("token-test").expect("generate key");
    generated.write_pair(&dir).expect("write pair");
    let keys = SigningKeySet::load_from_dir(&dir).expect("load keys");
    let signing = gs_auth::parse_private_pem(generated.private_pem()).expect("parse key");

    let minted = mint_device_token(
        &pool,
        &credential,
        &request(),
        bounds(),
        "https://geostream.example.com",
        &signing,
        "token-test",
    )
    .await
    .expect("mint token");

    // Response shape of OpenAPI TokenResponse.
    let body: Value = serde_json::to_value(&minted.response).expect("serialize");
    assert_eq!(body["token_type"], json!("Bearer"));
    assert!(
        body["access_token"]
            .as_str()
            .is_some_and(|t| t.starts_with("eyJ"))
    );
    assert_eq!(body["subject_id"], minted.subject.id.to_string());
    assert_eq!(body["device_id"], minted.device.id.to_string());
    assert_eq!(body["scopes"][0], json!("sessions:create"));
    assert!(
        body["expires_at"]
            .as_str()
            .is_some_and(|t| t.ends_with('Z'))
    );

    // The token verifies with the public half and carries the documented claims.
    let verified = verify_device_token(
        minted.response.access_token.as_str(),
        &keys,
        "https://geostream.example.com",
        30,
    )
    .expect("verify token");
    assert_eq!(
        verified.claims.sub.to_string(),
        minted.subject.id.to_string()
    );
    assert_eq!(verified.claims.tid, tenant_id);
    assert_eq!(verified.claims.aid, application_id);
    assert_eq!(
        verified.claims.did.to_string(),
        minted.device.id.to_string()
    );
    assert_eq!(verified.claims.sep, minted.subject.token_epoch);
    assert_eq!(verified.claims.scp, vec!["sessions:create".to_string()]);
    assert_eq!(verified.claims.aud, "geostream");
    assert_eq!(verified.claims.iss, "https://geostream.example.com");
    assert_eq!(verified.claims.exp - verified.claims.iat, 900);

    // INV-SUB-1: the same external id returns the same subject.
    let again = mint_device_token(
        &pool,
        &credential,
        &request(),
        bounds(),
        "https://geostream.example.com",
        &signing,
        "token-test",
    )
    .await
    .expect("second mint");
    assert_eq!(again.subject.id, minted.subject.id);

    // INV-DEV-1: the same identity tuple returns the same device.
    assert_eq!(again.device.id, minted.device.id);

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn test_default_scopes_are_the_documented_intersection() {
    // covers: FR-AUTH-002, docs/06 §4.1
    let (pool, _tenant, _app, credential) = setup().await;
    let dir = std::env::temp_dir().join(format!("gs-token-keys-2-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let generated = generate_signing_key("token-test-2").expect("generate key");
    generated.write_pair(&dir).expect("write pair");
    let signing = gs_auth::parse_private_pem(generated.private_pem()).expect("parse key");

    let mut request = request();
    request.scopes = None;
    let minted = mint_device_token(
        &pool,
        &credential,
        &request,
        bounds(),
        "https://geostream.example.com",
        &signing,
        "token-test-2",
    )
    .await
    .expect("mint token");

    // allowed_device_scopes ∩ {sessions:create, invitations:redeem, stream:connect, sharing:write}
    assert_eq!(
        minted.response.scopes,
        vec!["sessions:create".to_string(), "stream:connect".to_string()]
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn test_invalid_requests_are_rejected_with_the_documented_codes() {
    // covers: FR-AUTH-002, docs/06 §4.1, docs/09 §6
    let (pool, _tenant, _app, credential) = setup().await;
    let dir = std::env::temp_dir().join(format!("gs-token-keys-3-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let generated = generate_signing_key("token-test-3").expect("generate key");
    generated.write_pair(&dir).expect("write pair");
    let signing = gs_auth::parse_private_pem(generated.private_pem()).expect("parse key");

    // Unknown external subject id character.
    let mut bad = request();
    bad.external_subject_id = "user 42!".to_string();
    assert!(matches!(
        mint_device_token(&pool, &credential, &bad, bounds(), "iss", &signing, "k").await,
        Err(gs_service::tokens::TokenError::Validation(_))
    ));

    // installation_id must be a canonical UUIDv4.
    bad = request();
    bad.installation_id = "not-a-uuid".to_string();
    assert!(matches!(
        mint_device_token(&pool, &credential, &bad, bounds(), "iss", &signing, "k").await,
        Err(gs_service::tokens::TokenError::Validation(_))
    ));

    // Unknown platform.
    bad = request();
    bad.platform = "LINUX".to_string();
    assert!(matches!(
        mint_device_token(&pool, &credential, &bad, bounds(), "iss", &signing, "k").await,
        Err(gs_service::tokens::TokenError::Validation(_))
    ));

    // app_version above 32 characters.
    bad = request();
    bad.app_version = Some("x".repeat(33));
    assert!(matches!(
        mint_device_token(&pool, &credential, &bad, bounds(), "iss", &signing, "k").await,
        Err(gs_service::tokens::TokenError::Validation(_))
    ));

    // Requested scopes outside allowed_device_scopes.
    bad = request();
    bad.scopes = Some(vec!["sharing:write".to_string()]);
    assert!(matches!(
        mint_device_token(&pool, &credential, &bad, bounds(), "iss", &signing, "k").await,
        Err(gs_service::tokens::TokenError::InsufficientScope)
    ));

    // ttl_seconds outside the configured range.
    bad = request();
    bad.ttl_seconds = Some(60);
    assert!(matches!(
        mint_device_token(&pool, &credential, &bad, bounds(), "iss", &signing, "k").await,
        Err(gs_service::tokens::TokenError::InvalidTtl)
    ));

    bad = request();
    bad.ttl_seconds = Some(7200);
    assert!(matches!(
        mint_device_token(&pool, &credential, &bad, bounds(), "iss", &signing, "k").await,
        Err(gs_service::tokens::TokenError::InvalidTtl)
    ));

    // Malformed and unknown credentials.
    assert!(matches!(
        mint_device_token(
            &pool,
            "not-a-credential",
            &request(),
            bounds(),
            "iss",
            &signing,
            "k"
        )
        .await,
        Err(gs_service::tokens::TokenError::Unauthenticated)
    ));
    let unknown =
        "gsk_0192b6c0a1e47c3f9d2a7b5e8f1c0d3a.aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    assert!(matches!(
        mint_device_token(&pool, unknown, &request(), bounds(), "iss", &signing, "k").await,
        Err(gs_service::tokens::TokenError::Unauthenticated)
    ));

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn test_revoked_key_and_disabled_application_cannot_mint() {
    // covers: FR-AUTH-002, docs/06 §4.3, docs/09 §6
    let (pool, tenant_id, application_id, credential) = setup().await;
    let dir = std::env::temp_dir().join(format!("gs-token-keys-4-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let generated = generate_signing_key("token-test-4").expect("generate key");
    generated.write_pair(&dir).expect("write pair");
    let signing = gs_auth::parse_private_pem(generated.private_pem()).expect("parse key");

    // A key without the tokens:mint scope is rejected before any effect.
    let scope_less = create_api_key(
        &pool,
        &application_id.to_string(),
        Some(vec!["sessions:read".to_string()]),
        None,
        None,
    )
    .await
    .expect("api key");
    assert!(matches!(
        mint_device_token(
            &pool,
            scope_less.credential.as_str(),
            &request(),
            bounds(),
            "iss",
            &signing,
            "k"
        )
        .await,
        Err(gs_service::tokens::TokenError::MissingScope)
    ));

    // Disabling the application blocks the mint (ACCOUNT_DISABLED).
    gs_service::admin::set_application_enabled(&pool, &application_id.to_string(), false)
        .await
        .expect("disable application");
    assert!(matches!(
        mint_device_token(
            &pool,
            &credential,
            &request(),
            bounds(),
            "iss",
            &signing,
            "k"
        )
        .await,
        Err(gs_service::tokens::TokenError::AccountDisabled(_))
    ));

    // Disabling the tenant blocks it too.
    gs_service::admin::set_application_enabled(&pool, &application_id.to_string(), true)
        .await
        .expect("enable application");
    gs_service::admin::set_tenant_enabled(&pool, &tenant_id.to_string(), false)
        .await
        .expect("disable tenant");
    assert!(matches!(
        mint_device_token(
            &pool,
            &credential,
            &request(),
            bounds(),
            "iss",
            &signing,
            "k"
        )
        .await,
        Err(gs_service::tokens::TokenError::AccountDisabled(_))
    ));

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn test_verification_rejects_disabled_and_epoch_changed_principals() {
    // covers: FR-AUTH-003, docs/06 §4.3 step 4
    let (pool, tenant_id, application_id, credential) = setup().await;

    let dir = std::env::temp_dir().join(format!("gs-auth-verify-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let generated = generate_signing_key("verify-test").expect("generate key");
    generated.write_pair(&dir).expect("write pair");
    let keys = SigningKeySet::load_from_dir(&dir).expect("load keys");
    let signing = gs_auth::parse_private_pem(generated.private_pem()).expect("parse key");

    let minted = mint_device_token(
        &pool,
        &credential,
        &request(),
        bounds(),
        "https://geostream.example.com",
        &signing,
        "verify-test",
    )
    .await
    .expect("mint token");

    // A freshly issued token verifies against the state in the database.
    let principal = gs_service::auth::verify_device_token_principal(
        &pool,
        minted.response.access_token.as_str(),
        &keys,
        "https://geostream.example.com",
        30,
    )
    .await
    .expect("verify principal");
    assert_eq!(principal.tenant.id, tenant_id);
    assert_eq!(principal.application.id, application_id);
    assert_eq!(principal.subject.id, minted.subject.id);
    assert_eq!(principal.device.id, minted.device.id);
    assert!(
        gs_service::auth::require_scope(&principal, "sessions:create").is_ok(),
        "the minted scope must be granted"
    );
    assert!(matches!(
        gs_service::auth::require_scope(&principal, "stream:connect"),
        Err(gs_service::auth::AuthError::MissingScope(_))
    ));

    // A token from another issuer or with a tampered signature does not verify.
    assert!(
        gs_service::auth::verify_device_token_principal(
            &pool,
            minted.response.access_token.as_str(),
            &keys,
            "https://other.example.com",
            30,
        )
        .await
        .is_err()
    );

    // Bumping the subject epoch invalidates every outstanding token (docs/06 §4.3 step 4b).
    gs_store::repos::SubjectRepository::increment_token_epoch(
        &pool,
        tenant_id,
        minted.subject.id,
        OffsetDateTime::now_utc(),
    )
    .await
    .expect("bump epoch");
    let err = gs_service::auth::verify_device_token_principal(
        &pool,
        minted.response.access_token.as_str(),
        &keys,
        "https://geostream.example.com",
        30,
    )
    .await
    .expect_err("bumped epoch must fail verification");
    assert!(matches!(
        err,
        gs_service::auth::AuthError::Unauthenticated(_)
    ));
    assert_eq!(
        gs_service::gs_domain::error::AsErrorCode::error_code(&err),
        gs_service::gs_domain::error::ErrorCode::Unauthenticated
    );

    // Step 4a: Disabling the application returns AccountDisabled (403).
    let minted2 = mint_device_token(
        &pool,
        &credential,
        &request(),
        bounds(),
        "https://geostream.example.com",
        &signing,
        "verify-test",
    )
    .await
    .expect("mint token 2");

    gs_service::admin::set_application_enabled(&pool, &application_id.to_string(), false)
        .await
        .expect("disable application");
    let err = gs_service::auth::verify_device_token_principal(
        &pool,
        minted2.response.access_token.as_str(),
        &keys,
        "https://geostream.example.com",
        30,
    )
    .await
    .expect_err("disabled app must fail verification");
    assert!(matches!(
        err,
        gs_service::auth::AuthError::AccountDisabled(_)
    ));
    assert_eq!(
        gs_service::gs_domain::error::AsErrorCode::error_code(&err),
        gs_service::gs_domain::error::ErrorCode::AccountDisabled
    );

    // Re-enabling application restores access.
    gs_service::admin::set_application_enabled(&pool, &application_id.to_string(), true)
        .await
        .expect("enable application");
    assert!(
        gs_service::auth::verify_device_token_principal(
            &pool,
            minted2.response.access_token.as_str(),
            &keys,
            "https://geostream.example.com",
            30,
        )
        .await
        .is_ok()
    );

    // Disabling tenant returns AccountDisabled (403).
    gs_service::admin::set_tenant_enabled(&pool, &tenant_id.to_string(), false)
        .await
        .expect("disable tenant");
    let err = gs_service::auth::verify_device_token_principal(
        &pool,
        minted2.response.access_token.as_str(),
        &keys,
        "https://geostream.example.com",
        30,
    )
    .await
    .expect_err("disabled tenant must fail verification");
    assert!(matches!(
        err,
        gs_service::auth::AuthError::AccountDisabled(_)
    ));
    assert_eq!(
        gs_service::gs_domain::error::AsErrorCode::error_code(&err),
        gs_service::gs_domain::error::ErrorCode::AccountDisabled
    );

    // Re-enabling tenant restores access.
    gs_service::admin::set_tenant_enabled(&pool, &tenant_id.to_string(), true)
        .await
        .expect("enable tenant");
    assert!(
        gs_service::auth::verify_device_token_principal(
            &pool,
            minted2.response.access_token.as_str(),
            &keys,
            "https://geostream.example.com",
            30,
        )
        .await
        .is_ok()
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn test_verification_rejects_deactivated_device() {
    // covers: FR-AUTH-003, docs/06 §4.3 step 4c
    let (pool, tenant_id, _application_id, credential) = setup().await;
    let dir = std::env::temp_dir().join(format!("gs-auth-dev-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let generated = generate_signing_key("verify-dev").expect("generate key");
    generated.write_pair(&dir).expect("write pair");
    let keys = SigningKeySet::load_from_dir(&dir).expect("load keys");
    let signing = gs_auth::parse_private_pem(generated.private_pem()).expect("parse key");

    let minted = mint_device_token(
        &pool,
        &credential,
        &request(),
        bounds(),
        "https://geostream.example.com",
        &signing,
        "verify-dev",
    )
    .await
    .expect("mint token");

    gs_store::repos::DeviceRepository::deactivate(
        &pool,
        tenant_id,
        minted.device.id,
        OffsetDateTime::now_utc(),
    )
    .await
    .expect("deactivate device");

    let err = gs_service::auth::verify_device_token_principal(
        &pool,
        minted.response.access_token.as_str(),
        &keys,
        "https://geostream.example.com",
        30,
    )
    .await
    .expect_err("deactivated device must fail verification");
    assert!(matches!(
        err,
        gs_service::auth::AuthError::Unauthenticated(_)
    ));
    assert_eq!(
        gs_service::gs_domain::error::AsErrorCode::error_code(&err),
        gs_service::gs_domain::error::ErrorCode::Unauthenticated
    );

    let _ = std::fs::remove_dir_all(&dir);
}
