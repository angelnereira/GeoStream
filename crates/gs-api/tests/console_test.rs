//! `/v1/console/*` against PostgreSQL: registration, sign-in, organizations, projects and API
//! keys (T-F15-05, FR-CON-001 to FR-CON-012, ADR-0036). Every response, errors included, is
//! validated against the OpenAPI contract.
#![forbid(unsafe_code)]

mod common;

use std::net::TcpListener;
use std::sync::Arc;
use std::time::Duration;

use gs_api::{ConsoleApiState, RateLimitConfig, console_router};
use gs_service::console::{ConsoleAuth, ConsoleSettings, EmailBackend};
use gs_service::gs_auth::ConsoleKeys;
use gs_service::gs_auth::crypto::{CryptoRotator, DataCipher};
use gs_service::migrate::{connect_pool, run_migrations};
use reqwest::Method;
use serde_json::{Value, json};
use uuid::Uuid;

fn test_database_url() -> String {
    std::env::var("GS_DATABASE_URL").unwrap_or_else(|_| {
        "postgres://geostream:geostream_dev_password@127.0.0.1:5432/geostream_test".to_string()
    })
}

struct Harness {
    base: String,
    mailer: Arc<EmailBackend>,
    http: reqwest::Client,
    _server: tokio::task::JoinHandle<()>,
}

struct Reply {
    status: u16,
    headers: reqwest::header::HeaderMap,
    body: Value,
}

async fn harness_with(signup_rpm: u32, max_orgs: u32) -> Harness {
    let pool = connect_pool(&test_database_url()).await.expect("connect");
    run_migrations(&pool).await.expect("migrate");
    let mailer = Arc::new(EmailBackend::memory());
    let settings = ConsoleSettings {
        otp_length: 6,
        otp_ttl: Duration::from_secs(600),
        otp_max_attempts: 5,
        otp_cooldown: Duration::from_secs(60),
        session_ttl: Duration::from_secs(3600),
        email_daily_cap: 1_000_000,
        max_orgs_per_account: i64::from(max_orgs),
    };
    let auth = ConsoleAuth::new(
        pool.clone(),
        ConsoleKeys::new(&[7u8; 32]).expect("key"),
        settings,
        Arc::clone(&mailer),
    );
    let rotator = CryptoRotator::empty()
        .with_active(DataCipher::new("test_key", &[11u8; 32]).expect("cipher"));
    let state = ConsoleApiState {
        pool,
        auth: Arc::new(auth),
        ephemeral: gs_service::EphemeralBackend::memory(),
        bus: gs_service::BusBackend::memory(),
        // A prefix per harness keeps the counters of parallel tests apart.
        redis_key_prefix: format!("gs-test-{}", Uuid::new_v4().simple()),
        signup_rpm_per_ip: signup_rpm,
        max_orgs_per_account: max_orgs,
        crypto_rotator: Some(Arc::new(rotator)),
        idempotency_retention: Duration::from_secs(3600),
        rate_limit_config: RateLimitConfig::default(),
    };
    let port = TcpListener::bind("127.0.0.1:0")
        .expect("bind")
        .local_addr()
        .expect("addr")
        .port();
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
        .await
        .expect("bind");
    let app = console_router(state);
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });
    Harness {
        base: format!("http://127.0.0.1:{port}"),
        mailer,
        http: reqwest::Client::new(),
        _server: server,
    }
}

async fn harness() -> Harness {
    harness_with(1000, 3).await
}

impl Harness {
    /// Makes a call and checks the response against the contract for `op`.
    async fn call(
        &self,
        op: &str,
        method: Method,
        path: &str,
        token: Option<&str>,
        body: Option<Value>,
        extra: &[(&str, &str)],
    ) -> Reply {
        let spec = common::spec::load_spec();
        if let Some(body) = &body
            && body.is_object()
            && spec.operation(op).get("requestBody").is_some()
        {
            // Bodies that break the contract on purpose are sent with `raw`.
            spec.check_request(op, body);
        }
        self.raw(op, method, path, token, body, extra).await
    }

    async fn raw(
        &self,
        op: &str,
        method: Method,
        path: &str,
        token: Option<&str>,
        body: Option<Value>,
        extra: &[(&str, &str)],
    ) -> Reply {
        let mut request = self.http.request(method, format!("{}{path}", self.base));
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }
        for (name, value) in extra {
            request = request.header(*name, *value);
        }
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.expect("request");
        let status = response.status().as_u16();
        let headers = response.headers().clone();
        let content_type = headers
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let text = response.text().await.expect("text");
        let body = if text.is_empty() {
            Value::Null
        } else {
            serde_json::from_str(&text).expect("json body")
        };
        assert_eq!(
            headers.get("cache-control").and_then(|v| v.to_str().ok()),
            Some("no-store"),
            "{op}: every console response is no-store"
        );
        common::spec::load_spec().check_response(op, status, &content_type, &body);
        Reply {
            status,
            headers,
            body,
        }
    }

    /// Registers a new account and returns its session token and e-mail.
    async fn sign_up(&self) -> (String, String) {
        let email = format!("dev.{}@example.test", Uuid::new_v4().simple());
        let accepted = self
            .call(
                "requestConsoleCode",
                Method::POST,
                "/v1/console/auth/code",
                None,
                Some(json!({"email": email})),
                &[],
            )
            .await;
        assert_eq!(accepted.status, 202);
        let mail = self
            .mailer
            .sent()
            .into_iter()
            .rev()
            .find(|m| m.to == email)
            .expect("mail sent");
        let marker = "es: ";
        let start = mail.body.find(marker).expect("marker") + marker.len();
        let code = mail.body[start..start + 6].to_string();
        let session = self
            .call(
                "verifyConsoleCode",
                Method::POST,
                "/v1/console/auth/verify",
                None,
                Some(json!({"email": email, "code": code, "accept_terms": true})),
                &[],
            )
            .await;
        assert_eq!(session.status, 200, "{}", session.body);
        assert_eq!(session.body["created"], json!(true));
        (
            session.body["token"].as_str().expect("token").to_string(),
            email,
        )
    }

    async fn create_org(&self, token: &str, name: &str) -> String {
        let reply = self
            .call(
                "createConsoleOrganization",
                Method::POST,
                "/v1/console/organizations",
                Some(token),
                Some(json!({"name": name})),
                &[],
            )
            .await;
        assert_eq!(reply.status, 201, "{}", reply.body);
        assert_eq!(reply.body["role"], json!("OWNER"));
        reply.body["id"].as_str().expect("id").to_string()
    }

    async fn create_project(&self, token: &str, org: &str, name: &str) -> String {
        let reply = self
            .call(
                "createConsoleProject",
                Method::POST,
                &format!("/v1/console/organizations/{org}/projects"),
                Some(token),
                Some(json!({"name": name, "allowed_origins": ["https://app.example.test"]})),
                &[],
            )
            .await;
        assert_eq!(reply.status, 201, "{}", reply.body);
        reply.body["id"].as_str().expect("id").to_string()
    }
}

#[tokio::test]
async fn test_fr_con_001_a_person_registers_and_creates_an_api_key_from_scratch() {
    // covers: FR-CON-001, FR-CON-002, FR-CON-003, FR-CON-004
    let h = harness().await;
    let (token, email) = h.sign_up().await;

    let me = h
        .call(
            "getConsoleAccount",
            Method::GET,
            "/v1/console/me",
            Some(&token),
            None,
            &[],
        )
        .await;
    assert_eq!(me.status, 200);
    assert_eq!(me.body["email"], json!(email));

    let org = h.create_org(&token, "Acme Logistics").await;
    let listed = h
        .call(
            "listConsoleOrganizations",
            Method::GET,
            "/v1/console/organizations",
            Some(&token),
            None,
            &[],
        )
        .await;
    assert_eq!(listed.body["data"].as_array().map(Vec::len), Some(1));

    let project = h.create_project(&token, &org, "Fleet app").await;
    let key_path = format!("/v1/console/organizations/{org}/projects/{project}/keys");
    let created = h
        .call(
            "createConsoleApiKey",
            Method::POST,
            &key_path,
            Some(&token),
            Some(json!({"label": "backend", "scopes": ["sessions:read", "tokens:mint"]})),
            &[],
        )
        .await;
    assert_eq!(created.status, 201, "{}", created.body);
    let secret = created.body["secret"].as_str().expect("secret").to_string();
    assert!(secret.starts_with("gsk_"));
    let key_id = created.body["key"]["id"].as_str().expect("id").to_string();

    // The secret is shown once: listing the keys never carries it.
    let keys = h
        .call(
            "listConsoleApiKeys",
            Method::GET,
            &key_path,
            Some(&token),
            None,
            &[],
        )
        .await;
    assert_eq!(keys.status, 200);
    assert!(!keys.body.to_string().contains(&secret));
    assert_eq!(keys.body["data"][0]["revoked_at"], Value::Null);

    let revoked = h
        .call(
            "revokeConsoleApiKey",
            Method::DELETE,
            &format!("{key_path}/{key_id}"),
            Some(&token),
            None,
            &[],
        )
        .await;
    assert_eq!(revoked.status, 204);
    let keys = h
        .call(
            "listConsoleApiKeys",
            Method::GET,
            &key_path,
            Some(&token),
            None,
            &[],
        )
        .await;
    assert!(keys.body["data"][0]["revoked_at"].is_string());
}

#[tokio::test]
async fn test_fr_con_009_asking_for_a_code_answers_the_same_for_known_and_unknown_accounts() {
    // covers: FR-CON-009, ADR-0036 §2
    let h = harness().await;
    let (_token, known) = h.sign_up().await;
    let unknown = format!("nobody.{}@example.test", Uuid::new_v4().simple());
    let mut answers = Vec::new();
    for email in [known, unknown] {
        let reply = h
            .call(
                "requestConsoleCode",
                Method::POST,
                "/v1/console/auth/code",
                None,
                Some(json!({"email": email})),
                &[],
            )
            .await;
        answers.push((reply.status, reply.body));
    }
    assert_eq!(answers[0], answers[1]);
    assert_eq!(answers[0].0, 202);
}

#[tokio::test]
async fn test_a_wrong_code_and_a_missing_session_are_the_same_unauthenticated() {
    // covers: FR-CON-010, ADR-0036 §2
    let h = harness().await;
    let wrong = h
        .call(
            "verifyConsoleCode",
            Method::POST,
            "/v1/console/auth/verify",
            None,
            Some(json!({"email": "x@example.test", "code": "000000", "accept_terms": true})),
            &[],
        )
        .await;
    assert_eq!(wrong.status, 401);
    assert_eq!(wrong.body["code"], json!("UNAUTHENTICATED"));

    for (op, method, path) in [
        ("getConsoleAccount", Method::GET, "/v1/console/me"),
        (
            "listConsoleOrganizations",
            Method::GET,
            "/v1/console/organizations",
        ),
        ("logoutConsole", Method::POST, "/v1/console/auth/logout"),
    ] {
        let reply = h.call(op, method, path, None, None, &[]).await;
        assert_eq!(reply.status, 401, "{op}");
        assert_eq!(reply.body["code"], json!("UNAUTHENTICATED"), "{op}");
    }
    let bogus = format!("gcs_{}", "A".repeat(43));
    let reply = h
        .call(
            "getConsoleAccount",
            Method::GET,
            "/v1/console/me",
            Some(&bogus),
            None,
            &[],
        )
        .await;
    assert_eq!(reply.status, 401);
}

#[tokio::test]
async fn test_logout_revokes_the_session() {
    // covers: FR-CON-001, FR-CON-010
    let h = harness().await;
    let (token, _) = h.sign_up().await;
    let out = h
        .call(
            "logoutConsole",
            Method::POST,
            "/v1/console/auth/logout",
            Some(&token),
            None,
            &[],
        )
        .await;
    assert_eq!(out.status, 204);
    let after = h
        .call(
            "getConsoleAccount",
            Method::GET,
            "/v1/console/me",
            Some(&token),
            None,
            &[],
        )
        .await;
    assert_eq!(after.status, 401);
}

#[tokio::test]
async fn test_nfr_sec_004_another_account_cannot_see_or_touch_an_organization() {
    // covers: NFR-SEC-004, docs/06 §8.6 (404, never 403, for organizations of others)
    let h = harness().await;
    let (owner, _) = h.sign_up().await;
    let (intruder, _) = h.sign_up().await;
    let org = h.create_org(&owner, "Private").await;
    let project = h.create_project(&owner, &org, "Secret project").await;
    let key_path = format!("/v1/console/organizations/{org}/projects/{project}/keys");
    let created = h
        .call(
            "createConsoleApiKey",
            Method::POST,
            &key_path,
            Some(&owner),
            Some(json!({"label": "k", "scopes": ["sessions:read"]})),
            &[],
        )
        .await;
    let key_id = created.body["key"]["id"].as_str().expect("id").to_string();

    let org_path = format!("/v1/console/organizations/{org}");
    let project_path = format!("{org_path}/projects/{project}");
    let probes: Vec<(&str, Method, String, Option<Value>)> = vec![
        (
            "getConsoleOrganization",
            Method::GET,
            org_path.clone(),
            None,
        ),
        (
            "listConsoleProjects",
            Method::GET,
            format!("{org_path}/projects"),
            None,
        ),
        (
            "createConsoleProject",
            Method::POST,
            format!("{org_path}/projects"),
            Some(json!({"name": "x"})),
        ),
        ("getConsoleProject", Method::GET, project_path.clone(), None),
        (
            "updateConsoleProject",
            Method::PATCH,
            project_path.clone(),
            Some(json!({"status": "DISABLED"})),
        ),
        ("listConsoleApiKeys", Method::GET, key_path.clone(), None),
        (
            "createConsoleApiKey",
            Method::POST,
            key_path.clone(),
            Some(json!({"label": "k", "scopes": ["sessions:read"]})),
        ),
        (
            "revokeConsoleApiKey",
            Method::DELETE,
            format!("{key_path}/{key_id}"),
            None,
        ),
    ];
    for (op, method, path, body) in probes {
        let reply = h.call(op, method, &path, Some(&intruder), body, &[]).await;
        assert_eq!(reply.status, 404, "{op}: {}", reply.body);
        assert_eq!(reply.body["code"], json!("NOT_FOUND"), "{op}");
    }

    // Nothing the intruder tried changed anything.
    let project = h
        .call(
            "getConsoleProject",
            Method::GET,
            &project_path,
            Some(&owner),
            None,
            &[],
        )
        .await;
    assert_eq!(project.body["status"], json!("ACTIVE"));
    let keys = h
        .call(
            "listConsoleApiKeys",
            Method::GET,
            &key_path,
            Some(&owner),
            None,
            &[],
        )
        .await;
    assert_eq!(keys.body["data"][0]["revoked_at"], Value::Null);

    // A malformed id is indistinguishable from a missing one.
    let reply = h
        .call(
            "getConsoleOrganization",
            Method::GET,
            "/v1/console/organizations/not-an-id",
            Some(&owner),
            None,
            &[],
        )
        .await;
    assert_eq!(reply.status, 404);
}

#[tokio::test]
async fn test_a_key_cannot_be_revoked_through_another_project() {
    // covers: NFR-SEC-004 (the key must belong to the project in the path)
    let h = harness().await;
    let (token, _) = h.sign_up().await;
    let org = h.create_org(&token, "Two projects").await;
    let first = h.create_project(&token, &org, "First").await;
    let second = h.create_project(&token, &org, "Second").await;
    let created = h
        .call(
            "createConsoleApiKey",
            Method::POST,
            &format!("/v1/console/organizations/{org}/projects/{first}/keys"),
            Some(&token),
            Some(json!({"label": "k", "scopes": ["sessions:read"]})),
            &[],
        )
        .await;
    let key_id = created.body["key"]["id"].as_str().expect("id").to_string();
    let reply = h
        .call(
            "revokeConsoleApiKey",
            Method::DELETE,
            &format!("/v1/console/organizations/{org}/projects/{second}/keys/{key_id}"),
            Some(&token),
            None,
            &[],
        )
        .await;
    assert_eq!(reply.status, 404);
}

#[tokio::test]
async fn test_fr_con_005_an_api_key_needs_explicit_known_scopes() {
    // covers: FR-CON-005, docs/06 §3.4
    let h = harness().await;
    let (token, _) = h.sign_up().await;
    let org = h.create_org(&token, "Scopes").await;
    let project = h.create_project(&token, &org, "App").await;
    let path = format!("/v1/console/organizations/{org}/projects/{project}/keys");
    for body in [
        json!({"label": "k", "scopes": []}),
        json!({"label": "k", "scopes": ["admin:everything"]}),
        json!({"label": "", "scopes": ["sessions:read"]}),
        json!({"label": "k", "scopes": ["sessions:read"], "expires_in_days": 0}),
        json!({"label": "k", "scopes": ["sessions:read"], "owner": "x"}),
    ] {
        let reply = h
            .raw(
                "createConsoleApiKey",
                Method::POST,
                &path,
                Some(&token),
                Some(body.clone()),
                &[],
            )
            .await;
        assert_eq!(reply.status, 422, "{body}: {}", reply.body);
        assert_eq!(reply.body["code"], json!("VALIDATION_FAILED"));
    }
}

#[tokio::test]
async fn test_a_disabled_project_stops_issuing_keys_and_origins_are_validated() {
    // covers: FR-CON-004, FR-ADM-003
    let h = harness().await;
    let (token, _) = h.sign_up().await;
    let org = h.create_org(&token, "Lifecycle").await;
    let project = h.create_project(&token, &org, "App").await;
    let project_path = format!("/v1/console/organizations/{org}/projects/{project}");

    let bad_origin = h
        .raw(
            "updateConsoleProject",
            Method::PATCH,
            &project_path,
            Some(&token),
            Some(json!({"allowed_origins": ["https://app.example.test/path"]})),
            &[],
        )
        .await;
    assert_eq!(bad_origin.status, 422);

    let disabled = h
        .call(
            "updateConsoleProject",
            Method::PATCH,
            &project_path,
            Some(&token),
            Some(json!({"name": "Renamed", "status": "DISABLED"})),
            &[],
        )
        .await;
    assert_eq!(disabled.status, 200, "{}", disabled.body);
    assert_eq!(disabled.body["status"], json!("DISABLED"));
    assert_eq!(disabled.body["name"], json!("Renamed"));

    let key = h
        .raw(
            "createConsoleApiKey",
            Method::POST,
            &format!("{project_path}/keys"),
            Some(&token),
            Some(json!({"label": "k", "scopes": ["sessions:read"]})),
            &[],
        )
        .await;
    assert_eq!(key.status, 422);
}

#[tokio::test]
async fn test_fr_con_006_organizations_per_account_are_limited() {
    // covers: FR-CON-006 (GS_CONSOLE_MAX_ORGS_PER_ACCOUNT)
    let h = harness_with(1000, 2).await;
    let (token, _) = h.sign_up().await;
    h.create_org(&token, "One").await;
    h.create_org(&token, "Two").await;
    let third = h
        .call(
            "createConsoleOrganization",
            Method::POST,
            "/v1/console/organizations",
            Some(&token),
            Some(json!({"name": "Three"})),
            &[],
        )
        .await;
    assert_eq!(third.status, 429);
    assert_eq!(third.body["code"], json!("RATE_LIMITED"));
}

#[tokio::test]
async fn test_fr_con_012_sign_in_routes_are_limited_per_ip() {
    // covers: FR-CON-012 (GS_CONSOLE_SIGNUP_RPM_PER_IP)
    let h = harness_with(3, 3).await;
    let mut statuses = Vec::new();
    for _ in 0..5 {
        let reply = h
            .call(
                "requestConsoleCode",
                Method::POST,
                "/v1/console/auth/code",
                None,
                Some(json!({"email": format!("rl.{}@example.test", Uuid::new_v4().simple())})),
                &[],
            )
            .await;
        statuses.push(reply.status);
        if reply.status == 429 {
            assert!(reply.headers.contains_key("retry-after"));
        }
    }
    assert_eq!(statuses, vec![202, 202, 202, 429, 429]);
}

#[tokio::test]
async fn test_idempotency_key_replays_the_same_api_key_once() {
    // covers: docs/07 §6 (createConsoleApiKey accepts Idempotency-Key)
    let h = harness().await;
    let (token, _) = h.sign_up().await;
    let org = h.create_org(&token, "Idem").await;
    let project = h.create_project(&token, &org, "App").await;
    let path = format!("/v1/console/organizations/{org}/projects/{project}/keys");
    let key = format!("create-{}", Uuid::new_v4().simple());
    let body = json!({"label": "k", "scopes": ["sessions:read"]});

    let first = h
        .call(
            "createConsoleApiKey",
            Method::POST,
            &path,
            Some(&token),
            Some(body.clone()),
            &[("Idempotency-Key", &key)],
        )
        .await;
    let second = h
        .call(
            "createConsoleApiKey",
            Method::POST,
            &path,
            Some(&token),
            Some(body.clone()),
            &[("Idempotency-Key", &key)],
        )
        .await;
    assert_eq!(first.status, 201);
    assert_eq!(second.status, 201);
    assert_eq!(first.body, second.body);
    assert_eq!(
        second
            .headers
            .get("idempotent-replayed")
            .and_then(|v| v.to_str().ok()),
        Some("true")
    );
    let keys = h
        .call(
            "listConsoleApiKeys",
            Method::GET,
            &path,
            Some(&token),
            None,
            &[],
        )
        .await;
    assert_eq!(keys.body["data"].as_array().map(Vec::len), Some(1));

    let different = h
        .raw(
            "createConsoleApiKey",
            Method::POST,
            &path,
            Some(&token),
            Some(json!({"label": "other", "scopes": ["sessions:read"]})),
            &[("Idempotency-Key", &key)],
        )
        .await;
    assert_eq!(different.status, 422);
    assert_eq!(different.body["code"], json!("IDEMPOTENCY_KEY_REUSED"));
}

#[tokio::test]
async fn test_deleting_the_account_ends_its_sessions() {
    // covers: FR-CON-011
    let h = harness().await;
    let (token, _) = h.sign_up().await;
    let gone = h
        .call(
            "deleteConsoleAccount",
            Method::DELETE,
            "/v1/console/me",
            Some(&token),
            None,
            &[],
        )
        .await;
    assert_eq!(gone.status, 204);
    let after = h
        .call(
            "getConsoleAccount",
            Method::GET,
            "/v1/console/me",
            Some(&token),
            None,
            &[],
        )
        .await;
    assert_eq!(after.status, 401);
}
