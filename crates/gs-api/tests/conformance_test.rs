//! OpenAPI conformance, REST authorization matrix and tenant isolation
//! (T-F3-11, FR-API-001, FR-API-005, FR-API-006, NFR-SEC-004, docs/18 §5,
//! docs/06 §8 y §10).
//!
//! Every request body and every response (including errors) of every call made
//! by this suite is validated against `contracts/openapi/geostream.v1.yaml`,
//! converted to JSON by `redocly bundle` (docs/19 §6).
#![forbid(unsafe_code)]

mod common;

use std::collections::BTreeSet;
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::spec::{Spec, load_spec};
use gs_api::{RateLimitConfig, SessionApiState, StreamTicketConfig, sessions_router};
use gs_service::admin::{create_api_key, create_application, create_tenant};
use gs_service::auth::DeviceTokenKeys;
use gs_service::gs_auth::crypto::{CryptoRotator, DataCipher};
use gs_service::gs_auth::{generate_signing_key, parse_private_pem};
use gs_service::migrate::{connect_pool, run_migrations};
use gs_service::tokens::{MintedToken, TokenRequest, TokenTtlBounds, mint_device_token};
use serde_json::{Value, json};

/// Operations served by the router under test (the `mintToken` router and the
/// stream operations are covered by their own suites).
const OUT_OF_ROUTER: &[&str] = &["mintToken", "healthLive", "healthReady"];

// ---------------------------------------------------------------------------
// HTTP driver with contract validation and coverage tracking
// ---------------------------------------------------------------------------

struct Api {
    base: String,
    client: reqwest::Client,
    spec: &'static Spec,
    covered: Mutex<BTreeSet<(String, u16)>>,
}

struct Reply {
    status: u16,
    body: Value,
}

impl Reply {
    fn code(&self) -> &str {
        self.body["code"].as_str().unwrap_or("")
    }
}

impl Api {
    /// Calls `op_id` and validates request and response against OpenAPI.
    async fn call(
        &self,
        op_id: &str,
        token: Option<&str>,
        params: &[(&str, &str)],
        query: &str,
        body: Option<Value>,
        headers: &[(&str, &str)],
    ) -> Reply {
        let (method, template) = self.spec.operations().get(op_id).expect("op").clone();
        let mut path = template;
        for (name, value) in params {
            path = path.replace(&format!("{{{name}}}"), value);
        }
        let url = format!("{}{path}{query}", self.base);
        let mut req = self.client.request(method.parse().expect("method"), url);
        if let Some(t) = token {
            req = req.bearer_auth(t);
        }
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        if let Some(b) = &body {
            self.spec.check_request(op_id, b);
            req = req.json(b);
        }
        let resp = req.send().await.expect("send");
        let status = resp.status().as_u16();
        let content_type = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let text = resp.text().await.expect("text");
        let json_body = if text.is_empty() {
            Value::Null
        } else {
            serde_json::from_str(&text).unwrap_or_else(|_| panic!("{op_id}: non-JSON body {text}"))
        };
        self.spec
            .check_response(op_id, status, &content_type, &json_body);
        if status >= 400 {
            assert!(
                content_type.starts_with("application/problem+json"),
                "{op_id} {status}: errors must be application/problem+json (FR-API-001)"
            );
            assert!(
                json_body["request_id"].is_string() && json_body["code"].is_string(),
                "{op_id} {status}: Problem needs code and request_id"
            );
        }
        self.covered
            .lock()
            .expect("lock")
            .insert((op_id.to_string(), status));
        Reply {
            status,
            body: json_body,
        }
    }

    async fn simple(
        &self,
        op_id: &str,
        token: Option<&str>,
        params: &[(&str, &str)],
        body: Option<Value>,
    ) -> Reply {
        self.call(op_id, token, params, "", body, &[]).await
    }

    /// Asserts status (and optionally Problem code) and returns the reply.
    // The flat signature keeps each row of the authorization matrix on one line.
    #[allow(clippy::too_many_arguments)]
    async fn expect(
        &self,
        label: &str,
        op_id: &str,
        token: Option<&str>,
        params: &[(&str, &str)],
        body: Option<Value>,
        status: u16,
        code: Option<&str>,
    ) -> Reply {
        let reply = self.simple(op_id, token, params, body).await;
        assert_eq!(
            reply.status, status,
            "[{label}] {op_id}: expected {status}, got {} {}",
            reply.status, reply.body
        );
        if let Some(code) = code {
            assert_eq!(reply.code(), code, "[{label}] {op_id}: {}", reply.body);
        }
        reply
    }
}

// ---------------------------------------------------------------------------
// World: two tenants, principals and per-case resources
// ---------------------------------------------------------------------------

fn test_database_url() -> String {
    std::env::var("GS_DATABASE_URL").unwrap_or_else(|_| {
        "postgres://geostream:geostream_dev_password@127.0.0.1:5432/geostream_test".to_string()
    })
}

struct World {
    api: Api,
    _server: tokio::task::JoinHandle<()>,
    key_full: String,
    key_noscope: String,
    key_other_tenant: String,
    owner: MintedToken,
    reader: MintedToken,
    blind: MintedToken,
    outsider: MintedToken,
    noscope: MintedToken,
    other_tenant_dev: MintedToken,
    signing: gs_service::gs_auth::SigningKey,
}

fn tok(m: &MintedToken) -> &str {
    m.response.access_token.as_str()
}

struct Fixture {
    session_id: String,
    owner_member: String,
    reader_member: String,
    blind_member: String,
    invitation_id: String,
    invitation_code: String,
}

const FULL_APP_SCOPES: &[&str] = &[
    "tokens:mint",
    "sessions:write",
    "sessions:read",
    "members:write",
    "invitations:write",
    "subjects:manage",
    "locations:read",
];

async fn new_world() -> World {
    let pool = connect_pool(&test_database_url()).await.expect("connect");
    run_migrations(&pool).await.expect("migrate");
    let suffix = uuid::Uuid::new_v4().simple().to_string();

    let tenant_a = create_tenant(&pool, &format!("Conformance A {suffix}"))
        .await
        .expect("tenant a");
    let app_a = create_application(&pool, &tenant_a.id.to_string(), "conf-app-a", None, None)
        .await
        .expect("app a");
    let tenant_b = create_tenant(&pool, &format!("Conformance B {suffix}"))
        .await
        .expect("tenant b");
    let app_b = create_application(&pool, &tenant_b.id.to_string(), "conf-app-b", None, None)
        .await
        .expect("app b");

    let scopes: Vec<String> = FULL_APP_SCOPES.iter().map(|s| (*s).to_string()).collect();
    let key_full = create_api_key(
        &pool,
        &app_a.id.to_string(),
        Some(scopes.clone()),
        None,
        None,
    )
    .await
    .expect("key full");
    let key_noscope = create_api_key(
        &pool,
        &app_a.id.to_string(),
        Some(vec!["webhooks:manage".to_string()]),
        None,
        None,
    )
    .await
    .expect("key noscope");
    let key_b = create_api_key(&pool, &app_b.id.to_string(), Some(scopes), None, None)
        .await
        .expect("key b");

    let kid = "conformance-key";
    let generated = generate_signing_key(kid).expect("generate key");
    let signing = parse_private_pem(generated.private_pem()).expect("parse key");
    let mut keyset = DeviceTokenKeys::default();
    keyset.insert_signing_key(kid, signing.clone());

    let bounds = TokenTtlBounds {
        min_seconds: 300,
        default_seconds: 900,
        max_seconds: 3600,
    };
    let full_device_scopes: Vec<String> = [
        "sessions:create",
        "sharing:write",
        "invitations:redeem",
        "stream:connect",
    ]
    .iter()
    .map(|s| (*s).to_string())
    .collect();

    let mint = |credential: String, external: String, scopes: Vec<String>| {
        let pool = pool.clone();
        let signing = signing.clone();
        async move {
            let req = TokenRequest {
                external_subject_id: external,
                installation_id: uuid::Uuid::new_v4().to_string(),
                platform: "ANDROID".into(),
                app_version: None,
                sdk_version: None,
                scopes: Some(scopes),
                ttl_seconds: None,
            };
            mint_device_token(
                &pool,
                &credential,
                &req,
                bounds,
                "https://geostream.example.com",
                &signing,
                kid,
            )
            .await
            .expect("mint token")
        }
    };

    let key_a_cred = key_full.credential.to_string();
    let owner = mint(
        key_a_cred.clone(),
        format!("owner-{suffix}"),
        full_device_scopes.clone(),
    )
    .await;
    let reader = mint(
        key_a_cred.clone(),
        format!("reader-{suffix}"),
        full_device_scopes.clone(),
    )
    .await;
    let blind = mint(
        key_a_cred.clone(),
        format!("blind-{suffix}"),
        full_device_scopes.clone(),
    )
    .await;
    let outsider = mint(
        key_a_cred.clone(),
        format!("outsider-{suffix}"),
        full_device_scopes.clone(),
    )
    .await;
    let noscope = mint(
        key_a_cred,
        format!("noscope-{suffix}"),
        vec!["stream:connect".to_string()],
    )
    .await;
    let other_tenant_dev = mint(
        key_b.credential.to_string(),
        format!("other-{suffix}"),
        full_device_scopes,
    )
    .await;

    let raw = [42u8; 32];
    let rotator =
        CryptoRotator::empty().with_active(DataCipher::new("test_cipher", &raw).expect("cipher"));
    let state = SessionApiState {
        pool,
        signing_keys: Arc::new(keyset),
        issuer: "https://geostream.example.com".to_string(),
        leeway_seconds: 60,
        crypto_rotator: Some(Arc::new(rotator)),
        idempotency_retention: Duration::from_secs(3600),
        ephemeral: gs_service::InMemoryEphemeralStore::new(),
        bus: gs_service::InMemoryBus::new(),
        redis_key_prefix: format!("gs:conf:{suffix}"),
        stream_ticket: StreamTicketConfig::default(),
        rate_limit_config: RateLimitConfig {
            device_rpm: 1_000_000,
            app_rpm: 1_000_000,
            invite_redeem_rpm: 1_000_000,
            auth_failure_rpm: 1_000_000,
        },
    };
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    drop(listener);
    let tokio_listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
        .await
        .expect("bind");
    let router = sessions_router(state);
    let server = tokio::spawn(async move {
        axum::serve(tokio_listener, router).await.expect("serve");
    });

    World {
        api: Api {
            base: format!("http://127.0.0.1:{port}"),
            client: reqwest::Client::new(),
            spec: load_spec(),
            covered: Mutex::new(BTreeSet::new()),
        },
        _server: server,
        key_full: key_full.credential.to_string(),
        key_noscope: key_noscope.credential.to_string(),
        key_other_tenant: key_b.credential.to_string(),
        owner,
        reader,
        blind,
        outsider,
        noscope,
        other_tenant_dev,
        signing,
    }
}

fn caps(publish: bool, read: bool, invite: bool, manage: bool) -> Value {
    json!({ "publish": publish, "read": read, "invite": invite, "manage": manage })
}

impl World {
    /// Session owned by `owner` with `reader` (read only) and `blind` (no
    /// capability) as members plus one invitation issued by the application.
    async fn fixture(&self) -> Fixture {
        let api = &self.api;
        let session = api
            .expect(
                "fixture",
                "createSession",
                Some(&self.key_full),
                &[],
                Some(json!({
                    "mode": "ONE_TO_MANY",
                    "owner_external_subject_id": self.owner.subject.external_subject_id.clone().expect("external id"),
                })),
                201,
                None,
            )
            .await;
        let session_id = session.body["id"].as_str().expect("id").to_string();
        let mut member_ids = Vec::new();
        for (who, c) in [
            (&self.reader, caps(false, true, false, false)),
            (&self.blind, caps(false, false, false, false)),
        ] {
            let member = api
                .expect(
                    "fixture",
                    "addMember",
                    Some(&self.key_full),
                    &[("session_id", &session_id)],
                    Some(json!({
                        "external_subject_id": who.subject.external_subject_id.clone().expect("external id"),
                        "capabilities": c,
                    })),
                    201,
                    None,
                )
                .await;
            member_ids.push(member.body["id"].as_str().expect("member id").to_string());
        }
        let members = api
            .expect(
                "fixture",
                "listMembers",
                Some(&self.key_full),
                &[("session_id", &session_id)],
                None,
                200,
                None,
            )
            .await;
        let owner_member = members.body["data"]
            .as_array()
            .expect("data")
            .iter()
            .find(|m| m["role"] == "OWNER")
            .expect("owner member")["id"]
            .as_str()
            .expect("id")
            .to_string();
        let inv = self.invitation(&session_id).await;
        Fixture {
            session_id,
            owner_member,
            reader_member: member_ids[0].clone(),
            blind_member: member_ids[1].clone(),
            invitation_id: inv.0,
            invitation_code: inv.1,
        }
    }

    async fn invitation(&self, session_id: &str) -> (String, String) {
        let inv = self
            .api
            .expect(
                "fixture",
                "createInvitation",
                Some(&self.key_full),
                &[("session_id", session_id)],
                Some(json!({ "max_uses": 5 })),
                201,
                None,
            )
            .await;
        (
            inv.body["id"].as_str().expect("inv id").to_string(),
            inv.body["code"].as_str().expect("code").to_string(),
        )
    }
}

/// `If-Match` value for the initial version of a resource.
const V1: &str = "\"1\"";

async fn patch_session(
    w: &World,
    token: Option<&str>,
    session_id: &str,
    expected: (u16, Option<&str>),
    label: &str,
) -> Reply {
    let reply = w
        .api
        .call(
            "updateSession",
            token,
            &[("session_id", session_id)],
            "",
            Some(json!({ "max_members": 10 })),
            &[("If-Match", V1)],
        )
        .await;
    assert_eq!(
        reply.status, expected.0,
        "[{label}] updateSession: got {} {}",
        reply.status, reply.body
    );
    if let Some(code) = expected.1 {
        assert_eq!(reply.code(), code, "[{label}] {}", reply.body);
    }
    reply
}

async fn patch_member(
    w: &World,
    token: Option<&str>,
    session_id: &str,
    member_id: &str,
    expected: (u16, Option<&str>),
    label: &str,
) -> Reply {
    let reply = w
        .api
        .call(
            "updateMember",
            token,
            &[("session_id", session_id), ("member_id", member_id)],
            "",
            Some(json!({ "capabilities": caps(false, true, false, false) })),
            &[("If-Match", V1)],
        )
        .await;
    assert_eq!(
        reply.status, expected.0,
        "[{label}] updateMember: got {} {}",
        reply.status, reply.body
    );
    if let Some(code) = expected.1 {
        assert_eq!(reply.code(), code, "[{label}] {}", reply.body);
    }
    reply
}

// ---------------------------------------------------------------------------
// Authorization matrix (docs/06 §8.2, §10) + OpenAPI conformance
// ---------------------------------------------------------------------------

const UNAUTH: &str = "UNAUTHENTICATED";
const NO_SCOPE: &str = "INSUFFICIENT_SCOPE";
const DENIED: &str = "PERMISSION_DENIED";
const NOT_FOUND: &str = "NOT_FOUND";

#[tokio::test]
async fn test_fr_api_005_openapi_conformance_and_authorization_matrix() {
    let w = new_world().await;
    let api = &w.api;
    let key = Some(w.key_full.as_str());
    let nokey = Some(w.key_noscope.as_str());
    let key_b = Some(w.key_other_tenant.as_str());
    let owner = Some(tok(&w.owner));
    let reader = Some(tok(&w.reader));
    let blind = Some(tok(&w.blind));
    let outsider = Some(tok(&w.outsider));
    let noscope = Some(tok(&w.noscope));
    let dev_b = Some(tok(&w.other_tenant_dev));

    // ---- createSession ----------------------------------------------------
    let new_session = || Some(json!({ "mode": "ONE_TO_MANY" }));
    api.expect(
        "no credential",
        "createSession",
        None,
        &[],
        new_session(),
        401,
        Some(UNAUTH),
    )
    .await;
    api.expect(
        "key without scope",
        "createSession",
        nokey,
        &[],
        new_session(),
        403,
        Some(NO_SCOPE),
    )
    .await;
    api.expect(
        "key with scope",
        "createSession",
        key,
        &[],
        new_session(),
        201,
        None,
    )
    .await;
    api.expect(
        "device with sessions:create",
        "createSession",
        owner,
        &[],
        new_session(),
        201,
        None,
    )
    .await;
    api.expect(
        "device without sessions:create",
        "createSession",
        noscope,
        &[],
        new_session(),
        403,
        Some(NO_SCOPE),
    )
    .await;
    api.expect(
        "invalid body",
        "createSession",
        owner,
        &[],
        Some(json!({ "mode": "ONE_TO_MANY", "owner_external_subject_id": "someone" })),
        422,
        Some("VALIDATION_FAILED"),
    )
    .await;

    // ---- listSessions -----------------------------------------------------
    let f = w.fixture().await;
    let sid = [("session_id", f.session_id.as_str())];
    api.expect(
        "no credential",
        "listSessions",
        None,
        &[],
        None,
        401,
        Some(UNAUTH),
    )
    .await;
    api.expect(
        "key without scope",
        "listSessions",
        nokey,
        &[],
        None,
        403,
        Some(NO_SCOPE),
    )
    .await;
    api.expect("key with scope", "listSessions", key, &[], None, 200, None)
        .await;
    let own = api
        .expect("member device", "listSessions", owner, &[], None, 200, None)
        .await;
    assert!(
        own.body["data"]
            .as_array()
            .expect("data")
            .iter()
            .any(|s| s["id"] == f.session_id)
    );
    let other = api
        .expect(
            "non-member device",
            "listSessions",
            outsider,
            &[],
            None,
            200,
            None,
        )
        .await;
    assert!(
        !other.body["data"]
            .as_array()
            .expect("data")
            .iter()
            .any(|s| s["id"] == f.session_id),
        "non-member must not see the session in lists"
    );
    let other_tenant = api
        .expect(
            "other tenant key",
            "listSessions",
            key_b,
            &[],
            None,
            200,
            None,
        )
        .await;
    assert!(
        other_tenant.body["data"]
            .as_array()
            .expect("data")
            .is_empty()
    );

    // ---- getSession (FR-API-006) -------------------------------------------
    api.expect(
        "no credential",
        "getSession",
        None,
        &sid,
        None,
        401,
        Some(UNAUTH),
    )
    .await;
    api.expect(
        "key without scope",
        "getSession",
        nokey,
        &sid,
        None,
        403,
        Some(NO_SCOPE),
    )
    .await;
    api.expect("key with scope", "getSession", key, &sid, None, 200, None)
        .await;
    api.expect("owner", "getSession", owner, &sid, None, 200, None)
        .await;
    api.expect(
        "member without capability",
        "getSession",
        blind,
        &sid,
        None,
        200,
        None,
    )
    .await;
    api.expect(
        "other tenant key",
        "getSession",
        key_b,
        &sid,
        None,
        404,
        Some(NOT_FOUND),
    )
    .await;
    api.expect(
        "other tenant device",
        "getSession",
        dev_b,
        &sid,
        None,
        404,
        Some(NOT_FOUND),
    )
    .await;
    let outsider_reply = api
        .expect(
            "non-member",
            "getSession",
            outsider,
            &sid,
            None,
            404,
            Some(NOT_FOUND),
        )
        .await;
    let missing_id = format!("ses_{}", uuid::Uuid::new_v4().simple());
    let missing = api
        .expect(
            "unknown id",
            "getSession",
            outsider,
            &[("session_id", &missing_id)],
            None,
            404,
            Some(NOT_FOUND),
        )
        .await;
    assert_eq!(
        (
            outsider_reply.status,
            outsider_reply.body["title"].clone(),
            outsider_reply.body["type"].clone()
        ),
        (
            missing.status,
            missing.body["title"].clone(),
            missing.body["type"].clone()
        ),
        "FR-API-006: non-member and unknown id must be indistinguishable"
    );
    api.expect(
        "malformed id",
        "getSession",
        key,
        &[("session_id", "nope")],
        None,
        404,
        Some(NOT_FOUND),
    )
    .await;

    // ---- updateSession ------------------------------------------------------
    let fx = w.fixture().await;
    let id = fx.session_id.as_str();
    patch_session(&w, None, id, (401, Some(UNAUTH)), "no credential").await;
    patch_session(&w, nokey, id, (403, Some(NO_SCOPE)), "key without scope").await;
    patch_session(&w, key_b, id, (404, Some(NOT_FOUND)), "other tenant key").await;
    patch_session(&w, dev_b, id, (404, Some(NOT_FOUND)), "other tenant device").await;
    patch_session(&w, outsider, id, (404, Some(NOT_FOUND)), "non-member").await;
    patch_session(&w, reader, id, (403, Some(DENIED)), "member without manage").await;
    patch_session(&w, owner, id, (200, None), "member with manage").await;
    let fy = w.fixture().await;
    patch_session(&w, key, &fy.session_id, (200, None), "key with scope").await;

    // ---- closeSession -------------------------------------------------------
    let fc = w.fixture().await;
    let cid = [("session_id", fc.session_id.as_str())];
    api.expect(
        "no credential",
        "closeSession",
        None,
        &cid,
        None,
        401,
        Some(UNAUTH),
    )
    .await;
    api.expect(
        "key without scope",
        "closeSession",
        nokey,
        &cid,
        None,
        403,
        Some(NO_SCOPE),
    )
    .await;
    api.expect(
        "other tenant key",
        "closeSession",
        key_b,
        &cid,
        None,
        404,
        Some(NOT_FOUND),
    )
    .await;
    api.expect(
        "other tenant device",
        "closeSession",
        dev_b,
        &cid,
        None,
        404,
        Some(NOT_FOUND),
    )
    .await;
    api.expect(
        "non-member",
        "closeSession",
        outsider,
        &cid,
        None,
        404,
        Some(NOT_FOUND),
    )
    .await;
    api.expect(
        "member without manage",
        "closeSession",
        reader,
        &cid,
        None,
        403,
        Some(DENIED),
    )
    .await;
    api.expect(
        "member with manage",
        "closeSession",
        owner,
        &cid,
        None,
        200,
        None,
    )
    .await;
    let fc2 = w.fixture().await;
    api.expect(
        "key with scope",
        "closeSession",
        key,
        &[("session_id", fc2.session_id.as_str())],
        None,
        200,
        None,
    )
    .await;

    // ---- leaveSession -------------------------------------------------------
    let fl = w.fixture().await;
    let lid = [("session_id", fl.session_id.as_str())];
    api.expect(
        "no credential",
        "leaveSession",
        None,
        &lid,
        None,
        401,
        Some(UNAUTH),
    )
    .await;
    api.expect(
        "application is forbidden",
        "leaveSession",
        key,
        &lid,
        None,
        403,
        Some(DENIED),
    )
    .await;
    api.expect(
        "other tenant device",
        "leaveSession",
        dev_b,
        &lid,
        None,
        404,
        Some(NOT_FOUND),
    )
    .await;
    api.expect(
        "non-member",
        "leaveSession",
        outsider,
        &lid,
        None,
        404,
        Some(NOT_FOUND),
    )
    .await;
    api.expect(
        "active member",
        "leaveSession",
        reader,
        &lid,
        None,
        204,
        None,
    )
    .await;

    // ---- listMembers --------------------------------------------------------
    let fm = w.fixture().await;
    let mid = [("session_id", fm.session_id.as_str())];
    api.expect(
        "no credential",
        "listMembers",
        None,
        &mid,
        None,
        401,
        Some(UNAUTH),
    )
    .await;
    api.expect(
        "key without scope",
        "listMembers",
        nokey,
        &mid,
        None,
        403,
        Some(NO_SCOPE),
    )
    .await;
    api.expect(
        "other tenant key",
        "listMembers",
        key_b,
        &mid,
        None,
        404,
        Some(NOT_FOUND),
    )
    .await;
    api.expect(
        "other tenant device",
        "listMembers",
        dev_b,
        &mid,
        None,
        404,
        Some(NOT_FOUND),
    )
    .await;
    api.expect(
        "non-member",
        "listMembers",
        outsider,
        &mid,
        None,
        404,
        Some(NOT_FOUND),
    )
    .await;
    api.expect(
        "member without read",
        "listMembers",
        blind,
        &mid,
        None,
        403,
        Some(DENIED),
    )
    .await;
    api.expect(
        "member with read",
        "listMembers",
        reader,
        &mid,
        None,
        200,
        None,
    )
    .await;
    api.expect("key with scope", "listMembers", key, &mid, None, 200, None)
        .await;

    // ---- addMember ----------------------------------------------------------
    let add = |n: &str| {
        Some(
            json!({ "external_subject_id": format!("added-{n}-{}", uuid::Uuid::new_v4().simple()) }),
        )
    };
    api.expect(
        "no credential",
        "addMember",
        None,
        &mid,
        add("a"),
        401,
        Some(UNAUTH),
    )
    .await;
    api.expect(
        "key without scope",
        "addMember",
        nokey,
        &mid,
        add("b"),
        403,
        Some(NO_SCOPE),
    )
    .await;
    api.expect(
        "device is forbidden",
        "addMember",
        owner,
        &mid,
        add("c"),
        403,
        Some(DENIED),
    )
    .await;
    api.expect(
        "other tenant key",
        "addMember",
        key_b,
        &mid,
        add("d"),
        404,
        Some(NOT_FOUND),
    )
    .await;
    api.expect(
        "key with scope",
        "addMember",
        key,
        &mid,
        add("e"),
        201,
        None,
    )
    .await;

    // ---- getMember ----------------------------------------------------------
    let gm = |m: &'static str| [("session_id", fm.session_id.as_str()), ("member_id", m)];
    let owner_mid = fm.owner_member.as_str();
    let gm_owner = [
        ("session_id", fm.session_id.as_str()),
        ("member_id", owner_mid),
    ];
    api.expect(
        "no credential",
        "getMember",
        None,
        &gm_owner,
        None,
        401,
        Some(UNAUTH),
    )
    .await;
    api.expect(
        "key without scope",
        "getMember",
        nokey,
        &gm_owner,
        None,
        403,
        Some(NO_SCOPE),
    )
    .await;
    api.expect(
        "key with scope",
        "getMember",
        key,
        &gm_owner,
        None,
        200,
        None,
    )
    .await;
    api.expect(
        "other tenant key",
        "getMember",
        key_b,
        &gm_owner,
        None,
        404,
        Some(NOT_FOUND),
    )
    .await;
    api.expect(
        "other tenant device",
        "getMember",
        dev_b,
        &gm_owner,
        None,
        404,
        Some(NOT_FOUND),
    )
    .await;
    api.expect(
        "non-member",
        "getMember",
        outsider,
        &gm_owner,
        None,
        404,
        Some(NOT_FOUND),
    )
    .await;
    api.expect(
        "member with read",
        "getMember",
        reader,
        &gm_owner,
        None,
        200,
        None,
    )
    .await;
    api.expect(
        "member without read reads other",
        "getMember",
        blind,
        &gm_owner,
        None,
        403,
        Some(DENIED),
    )
    .await;
    api.expect(
        "member reads self via me",
        "getMember",
        blind,
        &gm("me"),
        None,
        200,
        None,
    )
    .await;

    // ---- updateMember -------------------------------------------------------
    let fu = w.fixture().await;
    let uid = fu.session_id.as_str();
    let target = fu.blind_member.as_str();
    patch_member(&w, None, uid, target, (401, Some(UNAUTH)), "no credential").await;
    patch_member(
        &w,
        nokey,
        uid,
        target,
        (403, Some(NO_SCOPE)),
        "key without scope",
    )
    .await;
    patch_member(
        &w,
        key_b,
        uid,
        target,
        (404, Some(NOT_FOUND)),
        "other tenant key",
    )
    .await;
    patch_member(
        &w,
        dev_b,
        uid,
        target,
        (404, Some(NOT_FOUND)),
        "other tenant device",
    )
    .await;
    patch_member(
        &w,
        outsider,
        uid,
        target,
        (404, Some(NOT_FOUND)),
        "non-member",
    )
    .await;
    patch_member(
        &w,
        reader,
        uid,
        target,
        (403, Some(DENIED)),
        "member without manage",
    )
    .await;
    patch_member(&w, owner, uid, target, (200, None), "member with manage").await;
    let fu2 = w.fixture().await;
    patch_member(
        &w,
        key,
        &fu2.session_id,
        &fu2.blind_member,
        (200, None),
        "key with scope",
    )
    .await;

    // ---- revokeMember -------------------------------------------------------
    let fr = w.fixture().await;
    let revoke =
        |label: &str, token: Option<&str>, member: &str, status: u16, code: Option<&str>| {
            let session = fr.session_id.clone();
            let member = member.to_string();
            let label = label.to_string();
            let token = token.map(str::to_string);
            let code = code.map(str::to_string);
            async move {
                api.expect(
                    &label,
                    "revokeMember",
                    token.as_deref(),
                    &[
                        ("session_id", session.as_str()),
                        ("member_id", member.as_str()),
                    ],
                    None,
                    status,
                    code.as_deref(),
                )
                .await
            }
        };
    revoke("no credential", None, &fr.blind_member, 401, Some(UNAUTH)).await;
    revoke(
        "key without scope",
        nokey,
        &fr.blind_member,
        403,
        Some(NO_SCOPE),
    )
    .await;
    revoke(
        "other tenant key",
        key_b,
        &fr.blind_member,
        404,
        Some(NOT_FOUND),
    )
    .await;
    revoke(
        "other tenant device",
        dev_b,
        &fr.blind_member,
        404,
        Some(NOT_FOUND),
    )
    .await;
    revoke(
        "non-member",
        outsider,
        &fr.blind_member,
        404,
        Some(NOT_FOUND),
    )
    .await;
    revoke(
        "member without manage",
        reader,
        &fr.owner_member,
        403,
        Some(DENIED),
    )
    .await;
    revoke("member with manage", owner, &fr.blind_member, 204, None).await;
    revoke("key with scope", key, &fr.reader_member, 204, None).await;

    // ---- setSharing ---------------------------------------------------------
    let fs = w.fixture().await;
    let share = |on: bool| Some(json!({ "sharing": on }));
    let me = [("session_id", fs.session_id.as_str()), ("member_id", "me")];
    api.expect(
        "no credential",
        "setSharing",
        None,
        &me,
        share(false),
        401,
        Some(UNAUTH),
    )
    .await;
    api.expect(
        "key without scope",
        "setSharing",
        nokey,
        &me,
        share(false),
        403,
        Some(NO_SCOPE),
    )
    .await;
    api.expect(
        "device without sharing:write",
        "setSharing",
        noscope,
        &me,
        share(false),
        403,
        Some(NO_SCOPE),
    )
    .await;
    api.expect(
        "other tenant device",
        "setSharing",
        dev_b,
        &me,
        share(false),
        404,
        Some(NOT_FOUND),
    )
    .await;
    api.expect(
        "non-member",
        "setSharing",
        outsider,
        &me,
        share(false),
        404,
        Some(NOT_FOUND),
    )
    .await;
    api.expect(
        "member without publish enables",
        "setSharing",
        reader,
        &me,
        share(true),
        403,
        Some(DENIED),
    )
    .await;
    api.expect(
        "member disables self",
        "setSharing",
        reader,
        &me,
        share(false),
        200,
        None,
    )
    .await;
    let owner_target = [
        ("session_id", fs.session_id.as_str()),
        ("member_id", fs.owner_member.as_str()),
    ];
    api.expect(
        "application cannot enable",
        "setSharing",
        key,
        &owner_target,
        share(true),
        403,
        Some(DENIED),
    )
    .await;
    api.expect(
        "application disables",
        "setSharing",
        key,
        &owner_target,
        share(false),
        200,
        None,
    )
    .await;
    api.expect(
        "other tenant key",
        "setSharing",
        key_b,
        &owner_target,
        share(false),
        404,
        Some(NOT_FOUND),
    )
    .await;

    // ---- createInvitation ---------------------------------------------------
    let fi = w.fixture().await;
    let iid = [("session_id", fi.session_id.as_str())];
    let inv = || Some(json!({ "max_uses": 3 }));
    api.expect(
        "no credential",
        "createInvitation",
        None,
        &iid,
        inv(),
        401,
        Some(UNAUTH),
    )
    .await;
    api.expect(
        "key without scope",
        "createInvitation",
        nokey,
        &iid,
        inv(),
        403,
        Some(NO_SCOPE),
    )
    .await;
    api.expect(
        "other tenant key",
        "createInvitation",
        key_b,
        &iid,
        inv(),
        404,
        Some(NOT_FOUND),
    )
    .await;
    api.expect(
        "other tenant device",
        "createInvitation",
        dev_b,
        &iid,
        inv(),
        404,
        Some(NOT_FOUND),
    )
    .await;
    api.expect(
        "non-member",
        "createInvitation",
        outsider,
        &iid,
        inv(),
        404,
        Some(NOT_FOUND),
    )
    .await;
    api.expect(
        "member without invite",
        "createInvitation",
        reader,
        &iid,
        inv(),
        403,
        Some(DENIED),
    )
    .await;
    api.expect(
        "member with invite",
        "createInvitation",
        owner,
        &iid,
        inv(),
        201,
        None,
    )
    .await;
    api.expect(
        "key with scope",
        "createInvitation",
        key,
        &iid,
        inv(),
        201,
        None,
    )
    .await;

    // ---- listInvitations ----------------------------------------------------
    api.expect(
        "no credential",
        "listInvitations",
        None,
        &iid,
        None,
        401,
        Some(UNAUTH),
    )
    .await;
    api.expect(
        "key without scope",
        "listInvitations",
        nokey,
        &iid,
        None,
        403,
        Some(NO_SCOPE),
    )
    .await;
    api.expect(
        "other tenant key",
        "listInvitations",
        key_b,
        &iid,
        None,
        404,
        Some(NOT_FOUND),
    )
    .await;
    api.expect(
        "other tenant device",
        "listInvitations",
        dev_b,
        &iid,
        None,
        404,
        Some(NOT_FOUND),
    )
    .await;
    api.expect(
        "non-member",
        "listInvitations",
        outsider,
        &iid,
        None,
        404,
        Some(NOT_FOUND),
    )
    .await;
    api.expect(
        "member without invite or manage",
        "listInvitations",
        reader,
        &iid,
        None,
        403,
        Some(DENIED),
    )
    .await;
    api.expect(
        "member with manage",
        "listInvitations",
        owner,
        &iid,
        None,
        200,
        None,
    )
    .await;
    api.expect(
        "key with scope",
        "listInvitations",
        key,
        &iid,
        None,
        200,
        None,
    )
    .await;

    // ---- revokeInvitation ---------------------------------------------------
    let fv = w.fixture().await;
    let rid = |i: &str| -> Vec<(String, String)> {
        vec![
            ("session_id".into(), fv.session_id.clone()),
            ("invitation_id".into(), i.to_string()),
        ]
    };
    let revoke_inv =
        |label: &str, token: Option<&str>, inv_id: &str, status: u16, code: Option<&str>| {
            let params = rid(inv_id);
            let label = label.to_string();
            let token = token.map(str::to_string);
            let code = code.map(str::to_string);
            async move {
                let p: Vec<(&str, &str)> = params
                    .iter()
                    .map(|(a, b)| (a.as_str(), b.as_str()))
                    .collect();
                api.expect(
                    &label,
                    "revokeInvitation",
                    token.as_deref(),
                    &p,
                    None,
                    status,
                    code.as_deref(),
                )
                .await
            }
        };
    revoke_inv("no credential", None, &fv.invitation_id, 401, Some(UNAUTH)).await;
    revoke_inv(
        "key without scope",
        nokey,
        &fv.invitation_id,
        403,
        Some(NO_SCOPE),
    )
    .await;
    revoke_inv(
        "other tenant key",
        key_b,
        &fv.invitation_id,
        404,
        Some(NOT_FOUND),
    )
    .await;
    revoke_inv(
        "other tenant device",
        dev_b,
        &fv.invitation_id,
        404,
        Some(NOT_FOUND),
    )
    .await;
    revoke_inv(
        "non-member",
        outsider,
        &fv.invitation_id,
        404,
        Some(NOT_FOUND),
    )
    .await;
    revoke_inv(
        "member without manage or issuer",
        reader,
        &fv.invitation_id,
        403,
        Some(DENIED),
    )
    .await;
    revoke_inv("member with manage", owner, &fv.invitation_id, 204, None).await;
    let (second_id, _) = w.invitation(&fv.session_id).await;
    revoke_inv("key with scope", key, &second_id, 204, None).await;

    // ---- redeemInvitation ---------------------------------------------------
    let fd = w.fixture().await;
    let code = |c: &str| Some(json!({ "code": c }));
    api.expect(
        "no credential",
        "redeemInvitation",
        None,
        &[],
        code(&fd.invitation_code),
        401,
        Some(UNAUTH),
    )
    .await;
    api.expect(
        "application is forbidden",
        "redeemInvitation",
        key,
        &[],
        code(&fd.invitation_code),
        403,
        Some(DENIED),
    )
    .await;
    api.expect(
        "device without invitations:redeem",
        "redeemInvitation",
        noscope,
        &[],
        code(&fd.invitation_code),
        403,
        Some(NO_SCOPE),
    )
    .await;
    api.expect(
        "other tenant device",
        "redeemInvitation",
        dev_b,
        &[],
        code(&fd.invitation_code),
        404,
        Some("INVITATION_INVALID"),
    )
    .await;
    api.expect(
        "unknown code",
        "redeemInvitation",
        outsider,
        &[],
        code("ZZZZZZZZZZZZ"),
        404,
        Some("INVITATION_INVALID"),
    )
    .await;
    api.expect(
        "device with scope",
        "redeemInvitation",
        outsider,
        &[],
        code(&fd.invitation_code),
        201,
        None,
    )
    .await;

    // ---- subjects and devices -----------------------------------------------
    let ext = w
        .owner
        .subject
        .external_subject_id
        .clone()
        .expect("external id")
        .clone();
    let find = |t: Option<&str>, label: &str, status: u16, code: Option<&str>| {
        let q = format!("?external_subject_id={ext}");
        let t = t.map(str::to_string);
        let label = label.to_string();
        let code = code.map(str::to_string);
        async move {
            let reply = api
                .call("findSubjects", t.as_deref(), &[], &q, None, &[])
                .await;
            assert_eq!(
                reply.status, status,
                "[{label}] findSubjects: {}",
                reply.body
            );
            if let Some(c) = code {
                assert_eq!(reply.code(), c, "[{label}] {}", reply.body);
            }
            reply
        }
    };
    find(None, "no credential", 401, Some(UNAUTH)).await;
    find(nokey, "key without scope", 403, Some(NO_SCOPE)).await;
    find(owner, "device is forbidden", 403, Some(DENIED)).await;
    let found = find(key, "key with scope", 200, None).await;
    assert_eq!(found.body["data"].as_array().expect("data").len(), 1);
    let none_for_b = find(key_b, "other tenant key", 200, None).await;
    assert!(none_for_b.body["data"].as_array().expect("data").is_empty());

    let owner_subject = w.owner.response.subject_id.as_str();
    let sj = [("subject_id", owner_subject)];
    api.expect(
        "no credential",
        "getSubject",
        None,
        &sj,
        None,
        401,
        Some(UNAUTH),
    )
    .await;
    api.expect(
        "key without scope",
        "getSubject",
        nokey,
        &sj,
        None,
        403,
        Some(NO_SCOPE),
    )
    .await;
    api.expect(
        "device is forbidden",
        "getSubject",
        owner,
        &sj,
        None,
        403,
        Some(DENIED),
    )
    .await;
    api.expect(
        "other tenant key",
        "getSubject",
        key_b,
        &sj,
        None,
        404,
        Some(NOT_FOUND),
    )
    .await;
    api.expect("key with scope", "getSubject", key, &sj, None, 200, None)
        .await;

    let victim = world_victim(&w).await;
    let vs = [("subject_id", victim.response.subject_id.as_str())];
    api.expect(
        "no credential",
        "revokeSubjectTokens",
        None,
        &vs,
        None,
        401,
        Some(UNAUTH),
    )
    .await;
    api.expect(
        "key without scope",
        "revokeSubjectTokens",
        nokey,
        &vs,
        None,
        403,
        Some(NO_SCOPE),
    )
    .await;
    api.expect(
        "device is forbidden",
        "revokeSubjectTokens",
        owner,
        &vs,
        None,
        403,
        Some(DENIED),
    )
    .await;
    api.expect(
        "other tenant key",
        "revokeSubjectTokens",
        key_b,
        &vs,
        None,
        404,
        Some(NOT_FOUND),
    )
    .await;
    api.expect(
        "key with scope",
        "revokeSubjectTokens",
        key,
        &vs,
        None,
        200,
        None,
    )
    .await;

    let own_dev = w.owner.response.device_id.as_str();
    let other_dev = w.reader.response.device_id.as_str();
    let dd = [("device_id", own_dev)];
    api.expect(
        "no credential",
        "getDevice",
        None,
        &dd,
        None,
        401,
        Some(UNAUTH),
    )
    .await;
    api.expect(
        "key without scope",
        "getDevice",
        nokey,
        &dd,
        None,
        403,
        Some(NO_SCOPE),
    )
    .await;
    api.expect(
        "other tenant key",
        "getDevice",
        key_b,
        &dd,
        None,
        404,
        Some(NOT_FOUND),
    )
    .await;
    api.expect(
        "other tenant device",
        "getDevice",
        dev_b,
        &dd,
        None,
        404,
        Some(NOT_FOUND),
    )
    .await;
    api.expect(
        "device reads another subject's device",
        "getDevice",
        owner,
        &[("device_id", other_dev)],
        None,
        404,
        Some(NOT_FOUND),
    )
    .await;
    api.expect(
        "device reads own device",
        "getDevice",
        owner,
        &dd,
        None,
        200,
        None,
    )
    .await;
    api.expect("key with scope", "getDevice", key, &dd, None, 200, None)
        .await;

    api.expect(
        "no credential",
        "deactivateDevice",
        None,
        &dd,
        None,
        401,
        Some(UNAUTH),
    )
    .await;
    api.expect(
        "key without scope",
        "deactivateDevice",
        nokey,
        &dd,
        None,
        403,
        Some(NO_SCOPE),
    )
    .await;
    api.expect(
        "other tenant key",
        "deactivateDevice",
        key_b,
        &dd,
        None,
        404,
        Some(NOT_FOUND),
    )
    .await;
    api.expect(
        "device deactivates another's device",
        "deactivateDevice",
        owner,
        &[("device_id", other_dev)],
        None,
        404,
        Some(NOT_FOUND),
    )
    .await;
    let own_throwaway = world_victim(&w).await;
    api.expect(
        "device deactivates own device",
        "deactivateDevice",
        Some(tok(&own_throwaway)),
        &[("device_id", own_throwaway.response.device_id.as_str())],
        None,
        204,
        None,
    )
    .await;
    let app_throwaway = world_victim(&w).await;
    api.expect(
        "key with scope",
        "deactivateDevice",
        key,
        &[("device_id", app_throwaway.response.device_id.as_str())],
        None,
        204,
        None,
    )
    .await;

    // ---- coverage of operations (docs/18 §5) -----------------------------------
    assert_operation_coverage(api);
}

/// Throwaway subject/device in tenant A (token creation consumes no fixture).
async fn world_victim(w: &World) -> MintedToken {
    let pool = connect_pool(&test_database_url()).await.expect("connect");
    let req = TokenRequest {
        external_subject_id: format!("victim-{}", uuid::Uuid::new_v4().simple()),
        installation_id: uuid::Uuid::new_v4().to_string(),
        platform: "ANDROID".into(),
        app_version: None,
        sdk_version: None,
        scopes: Some(vec!["stream:connect".to_string()]),
        ttl_seconds: None,
    };
    mint_device_token(
        &pool,
        &w.key_full,
        &req,
        TokenTtlBounds {
            min_seconds: 300,
            default_seconds: 900,
            max_seconds: 3600,
        },
        "https://geostream.example.com",
        &w.signing,
        "conformance-key",
    )
    .await
    .expect("mint victim")
}

/// Every implemented operation of phase ≤ F3 needs one success and one error
/// test (docs/18 §5).
fn assert_operation_coverage(api: &Api) {
    let covered = api.covered.lock().expect("lock").clone();
    let mut missing = Vec::new();
    for op_id in api.spec.operations().keys() {
        let phase = api.spec.operation(op_id)["x-gs-phase"]
            .as_str()
            .unwrap_or("");
        let in_scope = matches!(phase, "F2" | "F3");
        if !in_scope || OUT_OF_ROUTER.contains(&op_id.as_str()) {
            continue;
        }
        let ok = covered
            .iter()
            .any(|(o, s)| o == op_id && (200..300).contains(s));
        let err = covered.iter().any(|(o, s)| o == op_id && *s >= 400);
        if !ok || !err {
            missing.push(format!("{op_id} (success={ok}, error={err})"));
        }
    }
    assert!(
        missing.is_empty(),
        "operations without success and error tests: {missing:?}"
    );
}

// ---------------------------------------------------------------------------
// Tenant isolation matrix (NFR-SEC-004)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_nfr_sec_004_cross_tenant_matrix_covers_every_operation() {
    let w = new_world().await;
    let api = &w.api;
    let f = w.fixture().await;
    let foreign = [
        ("tenant B application", Some(w.key_other_tenant.as_str())),
        ("tenant B device", Some(tok(&w.other_tenant_dev))),
    ];
    let own_device = w.owner.response.device_id.clone();
    let subject = w.owner.response.subject_id.clone();

    // (operationId, path params, query, body, headers); application-forbidden
    // and device-forbidden outcomes are listed in `forbidden_for`.
    struct Probe {
        op: &'static str,
        params: Vec<(&'static str, String)>,
        query: String,
        body: Option<Value>,
        headers: Vec<(&'static str, &'static str)>,
    }
    let s = f.session_id.clone();
    let probes = vec![
        Probe {
            op: "getSession",
            params: vec![("session_id", s.clone())],
            query: String::new(),
            body: None,
            headers: vec![],
        },
        Probe {
            op: "updateSession",
            params: vec![("session_id", s.clone())],
            query: String::new(),
            body: Some(json!({ "max_members": 10 })),
            headers: vec![("If-Match", V1)],
        },
        Probe {
            op: "closeSession",
            params: vec![("session_id", s.clone())],
            query: String::new(),
            body: None,
            headers: vec![],
        },
        Probe {
            op: "leaveSession",
            params: vec![("session_id", s.clone())],
            query: String::new(),
            body: None,
            headers: vec![],
        },
        Probe {
            op: "listMembers",
            params: vec![("session_id", s.clone())],
            query: String::new(),
            body: None,
            headers: vec![],
        },
        Probe {
            op: "addMember",
            params: vec![("session_id", s.clone())],
            query: String::new(),
            body: Some(json!({ "external_subject_id": "intruder" })),
            headers: vec![],
        },
        Probe {
            op: "getMember",
            params: vec![
                ("session_id", s.clone()),
                ("member_id", f.owner_member.clone()),
            ],
            query: String::new(),
            body: None,
            headers: vec![],
        },
        Probe {
            op: "updateMember",
            params: vec![
                ("session_id", s.clone()),
                ("member_id", f.reader_member.clone()),
            ],
            query: String::new(),
            body: Some(json!({ "capabilities": caps(true, true, true, true) })),
            headers: vec![("If-Match", V1)],
        },
        Probe {
            op: "revokeMember",
            params: vec![
                ("session_id", s.clone()),
                ("member_id", f.reader_member.clone()),
            ],
            query: String::new(),
            body: None,
            headers: vec![],
        },
        Probe {
            op: "setSharing",
            params: vec![
                ("session_id", s.clone()),
                ("member_id", f.owner_member.clone()),
            ],
            query: String::new(),
            body: Some(json!({ "sharing": false })),
            headers: vec![],
        },
        Probe {
            op: "createInvitation",
            params: vec![("session_id", s.clone())],
            query: String::new(),
            body: Some(json!({})),
            headers: vec![],
        },
        Probe {
            op: "listInvitations",
            params: vec![("session_id", s.clone())],
            query: String::new(),
            body: None,
            headers: vec![],
        },
        Probe {
            op: "revokeInvitation",
            params: vec![
                ("session_id", s.clone()),
                ("invitation_id", f.invitation_id.clone()),
            ],
            query: String::new(),
            body: None,
            headers: vec![],
        },
        Probe {
            op: "redeemInvitation",
            params: vec![],
            query: String::new(),
            body: Some(json!({ "code": f.invitation_code.clone() })),
            headers: vec![],
        },
        Probe {
            op: "getSubject",
            params: vec![("subject_id", subject.clone())],
            query: String::new(),
            body: None,
            headers: vec![],
        },
        Probe {
            op: "revokeSubjectTokens",
            params: vec![("subject_id", subject.clone())],
            query: String::new(),
            body: None,
            headers: vec![],
        },
        Probe {
            op: "getDevice",
            params: vec![("device_id", own_device.clone())],
            query: String::new(),
            body: None,
            headers: vec![],
        },
        Probe {
            op: "deactivateDevice",
            params: vec![("device_id", own_device.clone())],
            query: String::new(),
            body: None,
            headers: vec![],
        },
    ];

    let mut exercised: BTreeSet<&str> = BTreeSet::new();
    for probe in &probes {
        exercised.insert(probe.op);
        let (app_auth, dev_auth) = {
            let auth = &api.spec.operation(probe.op)["x-gs-auth"];
            (
                auth["application"].as_str().unwrap_or("").to_string(),
                auth["device"].as_str().unwrap_or("").to_string(),
            )
        };
        let params: Vec<(&str, &str)> =
            probe.params.iter().map(|(k, v)| (*k, v.as_str())).collect();
        for (who, token) in foreign {
            let forbidden = if who.contains("application") {
                app_auth == "forbidden"
            } else {
                dev_auth == "forbidden"
            };
            let reply = api
                .call(
                    probe.op,
                    token,
                    &params,
                    &probe.query,
                    probe.body.clone(),
                    &probe.headers,
                )
                .await;
            if forbidden {
                assert_eq!(
                    (reply.status, reply.code()),
                    (403, DENIED),
                    "{who} on {}: principal is not allowed: {}",
                    probe.op,
                    reply.body
                );
            } else {
                let missing_scope_device = probe.op == "redeemInvitation";
                let expected = if missing_scope_device {
                    "INVITATION_INVALID"
                } else {
                    NOT_FOUND
                };
                assert_eq!(
                    (reply.status, reply.code()),
                    (404, expected),
                    "NFR-SEC-004: {who} reached tenant A resource through {}: {}",
                    probe.op,
                    reply.body
                );
            }
        }
    }

    // Lists never leak resources of another tenant.
    for (who, token) in foreign {
        let sessions = api.simple("listSessions", token, &[], None).await;
        assert_eq!(sessions.status, 200, "{who}: {}", sessions.body);
        assert!(
            sessions.body["data"].as_array().expect("data").is_empty(),
            "{who} sees foreign sessions"
        );
    }
    let q = format!(
        "?external_subject_id={}",
        w.owner
            .subject
            .external_subject_id
            .clone()
            .expect("external id")
    );
    let found = api
        .call(
            "findSubjects",
            Some(&w.key_other_tenant),
            &[],
            &q,
            None,
            &[],
        )
        .await;
    assert!(found.body["data"].as_array().expect("data").is_empty());

    // 100 % of the F3 operations served by this router are in the matrix.
    exercised.insert("listSessions");
    exercised.insert("findSubjects");
    exercised.insert("createSession");
    let expected: BTreeSet<&str> = api
        .spec
        .operations()
        .keys()
        .filter(|id| {
            matches!(api.spec.operation(id)["x-gs-phase"].as_str(), Some("F3"))
                && !OUT_OF_ROUTER.contains(&id.as_str())
        })
        .map(String::as_str)
        .collect();
    let missing: Vec<_> = expected.difference(&exercised).collect();
    assert!(
        missing.is_empty(),
        "F3 operations missing in isolation matrix: {missing:?}"
    );
}
