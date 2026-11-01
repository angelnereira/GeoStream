//! In-memory and test server harness for scenarios (docs/18 §6, contracts/README §5).
#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::net::TcpListener;
use std::sync::Arc;
use std::time::Duration;

use gs_api::{
    AcceptedWebSocket, AnalyticsState, ConnectionCounter, EdgeState, HistoryState, IngestState,
    PoolsLimits, PoolsState, RateLimitConfig, SessionApiState, SessionRunner, SigningKeyRef,
    StreamApiState, StreamTicketConfig, TokenState, ToolsLimits, ToolsState, analytics_router,
    edge_router, history_router, ingest_router, pools_router, sessions_router, stream_route,
    tools_router, v1_router,
};
use gs_auth::{SigningKey, generate_signing_key, parse_private_pem};
use gs_bus::BusBackend;
use gs_ephemeral::EphemeralBackend;
use gs_service::auth::DeviceTokenKeys;
use gs_service::authz::AuthzCache;
use gs_service::migrate::{PgPool, connect_pool, run_migrations};
use gs_service::tokens::{TokenRequest, TokenTtlBounds, mint_device_token};
use gs_stream::{AcceptedSession, StreamConfig, StreamState, run_session};
use serde_json::Value;

const ISSUER: &str = "https://auth.geostream.example.com";

/// Server running for scenario testing.
pub struct TestServer {
    pub http_base: String,
    pub ws_base: String,
    pub pool: PgPool,
    pub ephemeral: Arc<EphemeralBackend>,
    pub bus: Arc<BusBackend>,
    pub prefix: String,
    pub signing_key: SigningKey,
    pub kid: String,
    pub token_min_ttl: u32,
    _server_handle: tokio::task::JoinHandle<()>,
}

impl TestServer {
    /// Starts a test server instance with optional configuration overrides.
    pub async fn start(config_overrides: &BTreeMap<String, Value>) -> Result<Self, String> {
        let db_url = std::env::var("GS_DATABASE_URL")
            .or_else(|_| std::env::var("DATABASE_URL"))
            .unwrap_or_else(|_| {
                "postgres://geostream:geostream_dev_password@127.0.0.1:5432/geostream_test"
                    .to_string()
            });

        let pool = connect_pool(&db_url)
            .await
            .map_err(|e| format!("DB connect failed: {e}"))?;
        run_migrations(&pool)
            .await
            .map_err(|e| format!("DB migrations failed: {e}"))?;

        let run_id = uuid::Uuid::new_v4().to_string();
        let kid = format!("key-{run_id}");
        let generated =
            generate_signing_key(&kid).map_err(|e| format!("generate signing key failed: {e}"))?;
        let signing = parse_private_pem(generated.private_pem())
            .map_err(|e| format!("parse signing key failed: {e}"))?;

        let mut keyset = DeviceTokenKeys::default();
        keyset.insert_signing_key(&kid, signing.clone());
        let keyset_arc = Arc::new(keyset);

        let prefix = format!("gs:scn:{}", &run_id[..8]);
        let ephemeral = EphemeralBackend::memory();
        let bus = BusBackend::memory();
        let connections = ConnectionCounter::new();

        // Parse config overrides
        let token_min_ttl = config_overrides
            .get("GS_TOKEN_MIN_TTL_S")
            .and_then(|v| v.as_u64())
            .map(|n| n as u32)
            .unwrap_or(60);

        let auth_expiring_lead = config_overrides
            .get("GS_STREAM_AUTH_EXPIRING_LEAD_S")
            .and_then(|v| v.as_u64())
            .map(Duration::from_secs)
            .unwrap_or(Duration::from_secs(30));

        let idle_timeout = config_overrides
            .get("GS_STREAM_IDLE_TIMEOUT_S")
            .and_then(|v| v.as_u64())
            .map(Duration::from_secs)
            .unwrap_or(Duration::from_secs(60));

        let authz_cache_ttl = config_overrides
            .get("GS_AUTHZ_CACHE_TTL_MS")
            .and_then(|v| v.as_u64())
            .map(Duration::from_millis)
            .unwrap_or(Duration::from_millis(10_000));

        let stream_config = StreamConfig {
            authz_cache_ttl,
            idle_timeout,
            auth_expiring_lead,
            max_connections: 10_000,
            json_subprotocol_enabled: true,
            ..StreamConfig::default()
        };

        let stream_state = Arc::new(StreamState {
            pool: pool.clone(),
            ephemeral: ephemeral.clone(),
            bus: bus.clone(),
            authz: Arc::new(AuthzCache::new(stream_config.authz_cache_ttl)),
            fanout: Arc::new(gs_stream::Fanout::new(bus.clone(), prefix.clone())),
            redis_key_prefix: prefix.clone(),
            config: stream_config.clone(),
            signing_keys: keyset_arc.clone(),
            jwt_issuer: ISSUER.to_string(),
            jwt_leeway_seconds: 30,
        });

        let runner: SessionRunner = Arc::new(move |accepted: AcceptedWebSocket| {
            let state = Arc::clone(&stream_state);
            Box::pin(async move {
                run_session(
                    AcceptedSession {
                        subprotocol: accepted.subprotocol,
                        grant: accepted.grant,
                        request_id: accepted.request_id,
                        socket: accepted.socket,
                    },
                    state,
                )
                .await;
            })
        });

        let session_state = SessionApiState {
            pool: pool.clone(),
            signing_keys: keyset_arc.clone(),
            issuer: ISSUER.to_string(),
            leeway_seconds: 30,
            crypto_rotator: None,
            idempotency_retention: Duration::from_secs(3600),
            ephemeral: ephemeral.clone(),
            bus: bus.clone(),
            redis_key_prefix: prefix.clone(),
            stream_ticket: StreamTicketConfig {
                ttl: Duration::from_secs(30),
                app_max_connection_age: Duration::from_secs(3600),
                public_base_url: "https://geostream.example.com".to_string(),
            },
            rate_limit_config: RateLimitConfig::default(),
        };

        let stream_api_state = StreamApiState {
            pool: pool.clone(),
            ephemeral: ephemeral.clone(),
            redis_key_prefix: prefix.clone(),
            upgrade_rpm_per_ip: 10_000,
            max_connections: 10_000,
            max_frame_bytes: 16_384,
            connections: connections.clone(),
            json_subprotocol_enabled: true,
            session_runner: runner,
        };

        let token_state = TokenState {
            pool: pool.clone(),
            ttl_bounds: TokenTtlBounds {
                min_seconds: token_min_ttl,
                default_seconds: 900,
                max_seconds: 3600,
            },
            issuer: ISSUER.to_string(),
            signing_key: SigningKeyRef::new(signing.clone(), kid.clone()),
        };

        let listener = TcpListener::bind("127.0.0.1:0")
            .map_err(|e| format!("TcpListener bind failed: {e}"))?;
        let port = listener
            .local_addr()
            .map_err(|e| format!("local_addr failed: {e}"))?
            .port();
        drop(listener);

        let tokio_listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
            .await
            .map_err(|e| format!("tokio TcpListener bind failed: {e}"))?;

        // The E-track routers share the sessions state; mounting them here is
        // what lets an integration test exercise a real 200 path with the
        // tenant policy enabled, instead of only the 401 of an anonymous call.
        let router = sessions_router(session_state.clone())
            .merge(v1_router(token_state))
            .merge(stream_route(stream_api_state))
            .merge(tools_router(ToolsState {
                sessions: session_state.clone(),
                limits: ToolsLimits::default(),
            }))
            .merge(pools_router(PoolsState {
                sessions: session_state.clone(),
                limits: PoolsLimits::default(),
            }))
            .merge(analytics_router(AnalyticsState {
                sessions: session_state.clone(),
            }))
            .merge(history_router(HistoryState {
                sessions: session_state.clone(),
            }))
            .merge(edge_router(EdgeState {
                sessions: session_state.clone(),
            }))
            .merge(ingest_router(IngestState::with_defaults(session_state)));

        let server_handle = tokio::spawn(async move {
            let _ = axum::serve(tokio_listener, router).await;
        });

        Ok(Self {
            http_base: format!("http://127.0.0.1:{port}"),
            ws_base: format!("ws://127.0.0.1:{port}"),
            pool,
            ephemeral,
            bus,
            prefix,
            signing_key: signing,
            kid,
            token_min_ttl,
            _server_handle: server_handle,
        })
    }

    /// Mints a fresh device token for scenario testing.
    pub async fn mint_token(
        &self,
        bootstrap_credential: &str,
        external_subject_id: &str,
        installation_id: &str,
        platform: &str,
        scopes: Option<Vec<String>>,
        ttl_seconds: Option<u64>,
    ) -> Result<String, String> {
        let request = TokenRequest {
            external_subject_id: external_subject_id.to_string(),
            installation_id: installation_id.to_string(),
            platform: platform.to_string(),
            app_version: Some("1.0.0".to_string()),
            sdk_version: Some("0.1.0".to_string()),
            scopes,
            ttl_seconds: ttl_seconds.map(|s| s as u32),
        };

        let bounds = TokenTtlBounds {
            min_seconds: self.token_min_ttl,
            default_seconds: 900,
            max_seconds: 3600,
        };

        let minted = mint_device_token(
            &self.pool,
            bootstrap_credential,
            &request,
            bounds,
            ISSUER,
            &self.signing_key,
            &self.kid,
        )
        .await
        .map_err(|e| format!("mint_device_token error: {e}"))?;

        Ok(minted.response.access_token)
    }
}
