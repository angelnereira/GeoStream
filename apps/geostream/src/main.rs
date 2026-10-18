//! GeoStream server binary: serve, migrate, admin and version.
#![forbid(unsafe_code)]

pub mod admin;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand, ValueEnum};
use tracing::info;

use crate::admin::{AdminArgs, run_admin};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const GIT_COMMIT: &str = env!("GEOSTREAM_GIT_COMMIT");

/// Available server roles (FR-OPS-001, docs/04 §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "lowercase")]
pub enum Role {
    /// REST control plane and public HTTP endpoints.
    Api,
    /// WebSocket location streaming endpoint.
    Stream,
    /// Asynchronous jobs and background workers.
    Worker,
    /// Heavy batch analytics jobs (optional role, docs/04 §5).
    Analytics,
}

impl FromStr for Role {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "api" => Ok(Self::Api),
            "stream" => Ok(Self::Stream),
            "worker" => Ok(Self::Worker),
            "analytics" => Ok(Self::Analytics),
            other => {
                bail!("unknown role '{other}', expected one of: api, stream, worker, analytics")
            }
        }
    }
}

/// GeoStream CLI.
#[derive(Debug, Parser)]
#[command(
    name = "geostream",
    about = "GeoStream server binary: serve, migrate, admin and version",
    version = VERSION
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

/// GeoStream CLI subcommands (FR-OPS-001).
#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Run the GeoStream server process.
    Serve(ServeArgs),
    /// Run database migrations.
    Migrate(MigrateArgs),
    /// Administrative operations.
    Admin(AdminArgs),
    /// Print version and build commit information.
    Version,
}

/// Arguments for `serve` subcommand.
#[derive(Debug, Args)]
pub struct ServeArgs {
    /// Active roles (comma-separated or repeated: api, stream, worker, analytics). Defaults to api,stream,worker.
    #[arg(long, value_delimiter = ',')]
    pub roles: Option<Vec<Role>>,

    /// Path to configuration file.
    #[arg(long)]
    pub config: Option<PathBuf>,

    /// HTTP listener address (defaults to GS_HTTP_ADDR or 0.0.0.0:8080).
    #[arg(long, env = "GS_HTTP_ADDR", default_value = "0.0.0.0:8080")]
    pub http_addr: SocketAddr,

    /// Database connection URL (defaults to GS_DATABASE_URL).
    #[arg(long, env = "GS_DATABASE_URL")]
    pub database_url: Option<String>,

    /// Auto-migrate database on startup (allowed only in dev).
    #[arg(long, env = "GS_AUTO_MIGRATE", default_value = "false")]
    pub auto_migrate: bool,

    /// Active environment (dev, test, prod). Defaults to dev.
    #[arg(long, env = "GS_ENV", default_value = "dev")]
    pub env: String,
}

/// Arguments for `migrate` subcommand.
#[derive(Debug, Args)]
pub struct MigrateArgs {
    /// Show migration plan without applying changes.
    #[arg(long)]
    pub dry_run: bool,

    /// Database connection URL (defaults to GS_DATABASE_URL).
    #[arg(long, env = "GS_DATABASE_URL")]
    pub database_url: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    run(cli).await
}

/// Run the parsed CLI command.
pub async fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Commands::Version => {
            println!("geostream {} (commit: {})", VERSION, GIT_COMMIT);
            Ok(())
        }
        Commands::Migrate(args) => {
            let db_url = args
                .database_url
                .or_else(|| std::env::var("GS_DATABASE_URL").ok())
                .context("missing database URL: provide --database-url or set GS_DATABASE_URL")?;

            let pool = gs_api::migrate::connect_pool(&db_url)
                .await
                .context("failed to connect to database for migration")?;

            if args.dry_run {
                let status = gs_api::migrate::plan_migrations(&pool)
                    .await
                    .context("failed to inspect migration plan")?;
                println!(
                    "Migration plan (dry-run): current version: {:?}, expected version: {}, pending migrations: {}",
                    status.current_version, status.expected_version, status.pending_count
                );
            } else {
                gs_api::migrate::run_migrations(&pool)
                    .await
                    .context("failed to apply migrations")?;
                let latest = gs_api::migrate::expected_schema_version();
                println!("Successfully applied migrations up to version {latest}");
            }
            Ok(())
        }
        Commands::Admin(args) => run_admin(args).await,
        Commands::Serve(args) => run_serve(args).await,
    }
}

/// Run the server listener (docs/04 §5, DR-20261012-04, DR-20261013-01).
async fn run_serve(args: ServeArgs) -> Result<()> {
    // 1. Validate auto_migrate environment rule immediately (docs/11 §4, FR-OPS-004).
    if args.auto_migrate && args.env != "dev" {
        bail!(
            "GS_AUTO_MIGRATE=true is only allowed in GS_ENV=dev (current env: {})",
            args.env
        );
    }

    // 2. Initialise structured logging from `gs-observability` (DR-20261012-04, NFR-OBS-001).
    let unsafe_locations = std::env::var("GS_LOG_UNSAFE_LOCATIONS")
        .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
        .unwrap_or(false);
    gs_observability::logging_policy(&args.env, unsafe_locations)
        .context("logging policy validation failed")?;
    gs_observability::init_from_env().context("failed to initialize structured logging")?;
    gs_observability::record_build_info(VERSION, GIT_COMMIT);

    // 3. Build environment map with CLI overrides for `gs_config::ServerConfig` (DR-20261013-01).
    let mut env_map: HashMap<String, String> = std::env::vars().collect();
    if let Some(config_path) = &args.config {
        env_map.insert(
            "GS_CONFIG_FILE".to_string(),
            config_path.to_string_lossy().into_owned(),
        );
    }
    env_map.insert("GS_ENV".to_string(), args.env.clone());
    env_map.insert("GS_HTTP_ADDR".to_string(), args.http_addr.to_string());
    env_map.insert("GS_AUTO_MIGRATE".to_string(), args.auto_migrate.to_string());
    if let Some(db_url) = &args.database_url {
        env_map.insert("GS_DATABASE_URL".to_string(), db_url.clone());
    }

    // Try loading full `ServerConfig` when all required keys exist; if running in lightweight
    // dev/test mode without JWT key dir, supply ephemeral defaults in `env_map` if not in prod.
    let mut ephemeral_key_dir: Option<PathBuf> = None;
    if args.env != "prod"
        && env_map.contains_key("GS_DATABASE_URL")
        && (!env_map.contains_key("GS_JWT_KEYS_DIR") || !env_map.contains_key("GS_JWT_ACTIVE_KID"))
        && !env_map.contains_key("GS_CONFIG_FILE")
    {
        let dir = std::env::temp_dir().join(format!(
            "geostream-dev-keys-{}-{}",
            std::process::id(),
            gs_api::gs_service::gs_domain::ids::RequestId::new()
        ));
        std::fs::create_dir_all(&dir).context("failed to create ephemeral dev JWT key dir")?;
        let kid = "dev-key-1";
        let generated = gs_api::gs_service::gs_auth::generate_signing_key(kid)
            .context("failed to generate ephemeral dev signing key")?;
        std::fs::write(dir.join(format!("{kid}.key.pem")), generated.private_pem())
            .context("failed to write ephemeral dev private key")?;
        std::fs::write(dir.join(format!("{kid}.pub.pem")), &generated.public_pem)
            .context("failed to write ephemeral dev public key")?;
        env_map
            .entry("GS_JWT_KEYS_DIR".to_string())
            .or_insert_with(|| dir.to_string_lossy().into_owned());
        env_map
            .entry("GS_JWT_ACTIVE_KID".to_string())
            .or_insert_with(|| kid.to_string());
        ephemeral_key_dir = Some(dir);
    }

    let server_config =
        if env_map.contains_key("GS_DATABASE_URL") || env_map.contains_key("GS_CONFIG_FILE") {
            Some(
                gs_config::ServerConfig::load_from_map(&env_map)
                    .context("failed to load ServerConfig")?,
            )
        } else {
            None
        };

    let roles = args
        .roles
        .unwrap_or_else(|| vec![Role::Api, Role::Stream, Role::Worker]);
    let http_addr = server_config
        .as_ref()
        .map(|c| c.http_addr)
        .unwrap_or(args.http_addr);
    let max_body_bytes = server_config
        .as_ref()
        .map(|c| c.rest_max_body_bytes)
        .unwrap_or(gs_api::DEFAULT_MAX_BODY_BYTES);

    let database_url = server_config
        .as_ref()
        .map(|c| c.database_url.expose_secret().to_string())
        .or_else(|| env_map.get("GS_DATABASE_URL").cloned());

    // 4. Connect to PostgreSQL, verify schema version (or auto-migrate in dev), and build routers.
    let mut app = gs_api::http_router_with_readiness(
        max_body_bytes,
        readiness_probe(database_url.as_deref()),
    );

    if let Some(cfg) = &server_config {
        let db_url = cfg.database_url.expose_secret();
        let pool = gs_api::migrate::connect_pool(db_url)
            .await
            .context("failed to connect to database on server startup")?;

        if cfg.auto_migrate {
            info!("auto-migrating database schema in dev environment");
            gs_api::migrate::run_migrations(&pool)
                .await
                .context("auto-migrate failed during server startup")?;
        } else {
            gs_api::migrate::check_schema_version(&pool).await.context(
                "database schema version check failed on startup (run 'geostream migrate')",
            )?;
            info!(
                version = gs_api::migrate::expected_schema_version(),
                "database schema version verified"
            );
        }

        // Load JWT signing keys
        let mut keyset =
            gs_api::gs_service::gs_auth::jwt::SigningKeySet::load_from_dir(&cfg.jwt_keys_dir)
                .context("failed to load JWT signing keys from GS_JWT_KEYS_DIR")?;
        keyset
            .set_active_kid(&cfg.jwt_active_kid)
            .context("failed to set active JWT signing key GS_JWT_ACTIVE_KID")?;
        let (_, active_signing_key) = keyset
            .active_signing_key()
            .context("active signing key not present in keyset")?;
        let active_signing_key = active_signing_key.clone();
        let keyset_arc = Arc::new(keyset);

        // Optional data encryption rotator for idempotency responses
        let crypto_rotator = cfg.data_encryption_key.as_ref().and_then(|active| {
            let active_cipher =
                gs_api::gs_service::gs_auth::crypto::cipher_from_config_key(active).ok()?;
            let mut rotator = gs_api::gs_service::gs_auth::crypto::CryptoRotator::empty()
                .with_active(active_cipher);
            for old in &cfg.data_encryption_keys_old {
                if let Ok(old_cipher) =
                    gs_api::gs_service::gs_auth::crypto::cipher_from_config_key(old)
                {
                    rotator = rotator.with_old(old_cipher);
                }
            }
            Some(Arc::new(rotator))
        });

        let ephemeral = gs_api::gs_service::InMemoryEphemeralStore::new();
        let bus = gs_api::gs_service::InMemoryBus::new();
        let prefix = cfg.redis_key_prefix.clone();

        if roles.contains(&Role::Api) {
            let session_state = gs_api::SessionApiState {
                pool: pool.clone(),
                signing_keys: keyset_arc.clone(),
                issuer: cfg.jwt_issuer.clone(),
                leeway_seconds: cfg.jwt_leeway_s,
                crypto_rotator,
                idempotency_retention: Duration::from_secs(
                    u64::from(cfg.retention_idempotency_h) * 3600,
                ),
                ephemeral: ephemeral.clone(),
                bus: bus.clone(),
                redis_key_prefix: prefix.clone(),
                stream_ticket: gs_api::StreamTicketConfig {
                    ttl: Duration::from_secs(cfg.ticket_ttl_s),
                    app_max_connection_age: Duration::from_secs(
                        cfg.stream_app_max_connection_age_s,
                    ),
                    public_base_url: cfg.public_base_url.clone(),
                },
                rate_limit_config: gs_api::RateLimitConfig {
                    device_rpm: cfg.rest_rate_limit_device_rpm,
                    app_rpm: cfg.rest_rate_limit_app_rpm,
                    invite_redeem_rpm: cfg.invite_redeem_rpm,
                    auth_failure_rpm: cfg.auth_failure_rpm_per_ip,
                },
            };

            let token_state = gs_api::TokenState {
                pool: pool.clone(),
                ttl_bounds: gs_api::gs_service::TokenTtlBounds {
                    min_seconds: cfg.token_min_ttl_s as u32,
                    default_seconds: cfg.token_default_ttl_s as u32,
                    max_seconds: cfg.token_max_ttl_s as u32,
                },
                issuer: cfg.jwt_issuer.clone(),
                signing_key: gs_api::SigningKeyRef::new(
                    active_signing_key,
                    cfg.jwt_active_kid.clone(),
                ),
            };

            app = app
                .merge(gs_api::sessions_router(session_state.clone()))
                .merge(gs_api::v1_router(token_state))
                .merge(gs_api::tools_router(gs_api::ToolsState {
                    sessions: session_state.clone(),
                    limits: gs_api::ToolsLimits::default(),
                }))
                .merge(gs_api::pools_router(gs_api::PoolsState {
                    sessions: session_state.clone(),
                    limits: gs_api::PoolsLimits::default(),
                }))
                .merge(gs_api::ingest_router(gs_api::IngestState {
                    sessions: session_state.clone(),
                }))
                .merge(gs_api::analytics_router(gs_api::AnalyticsState {
                    sessions: session_state,
                }));
        }

        if roles.contains(&Role::Stream) {
            let stream_config = gs_stream::StreamConfig {
                json_subprotocol_enabled: cfg.stream_json_enabled,
                max_connections: cfg.stream_max_connections,
                max_frame_bytes: cfg.stream_max_frame_bytes,
                hello_timeout: Duration::from_millis(cfg.stream_hello_timeout_ms),
                heartbeat_interval: Duration::from_millis(cfg.stream_heartbeat_interval_ms),
                idle_timeout: Duration::from_millis(cfg.stream_idle_timeout_ms),
                max_subscriptions: cfg.stream_max_subscriptions,
                max_subscriptions_app: cfg.stream_max_subscriptions_app,
                max_protocol_errors: cfg.stream_max_protocol_errors,
                max_inbound_msgs_per_s: cfg.stream_max_inbound_msgs_per_s,
                abuse_window: Duration::from_secs(cfg.stream_abuse_window_s),
                max_event_age: Duration::from_millis(cfg.location_max_event_age_ms),
                max_future_skew_ms: cfg.location_max_future_skew_ms as i64,
                authz_cache_ttl: Duration::from_millis(cfg.authz_cache_ttl_ms),
                outbound_control_queue: cfg.stream_outbound_control_queue,
                instance_id: cfg.instance_id.clone(),
                presence_ttl: Duration::from_secs(cfg.presence_ttl_s),
                offline_grace: Duration::from_millis(cfg.stream_offline_grace_ms),
                auth_expiring_lead: Duration::from_secs(cfg.stream_auth_expiring_lead_s),
            };

            let stream_state = Arc::new(gs_stream::StreamState {
                pool: pool.clone(),
                ephemeral: ephemeral.clone(),
                bus: bus.clone(),
                authz: Arc::new(gs_api::gs_service::AuthzCache::new(
                    stream_config.authz_cache_ttl,
                )),
                fanout: Arc::new(gs_stream::Fanout::new(bus.clone(), prefix.clone())),
                redis_key_prefix: prefix.clone(),
                config: stream_config,
                signing_keys: keyset_arc.clone(),
                jwt_issuer: cfg.jwt_issuer.clone(),
                jwt_leeway_seconds: cfg.jwt_leeway_s,
            });

            let runner: gs_api::SessionRunner =
                Arc::new(move |accepted: gs_api::AcceptedWebSocket| {
                    let state = Arc::clone(&stream_state);
                    Box::pin(async move {
                        gs_stream::run_session(
                            gs_stream::AcceptedSession {
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

            let stream_api_state = gs_api::StreamApiState {
                pool: pool.clone(),
                ephemeral,
                redis_key_prefix: prefix,
                upgrade_rpm_per_ip: cfg.stream_upgrade_rpm_per_ip,
                max_connections: cfg.stream_max_connections,
                max_frame_bytes: cfg.stream_max_frame_bytes,
                connections: gs_api::ConnectionCounter::new(),
                json_subprotocol_enabled: cfg.stream_json_enabled,
                session_runner: runner,
            };

            app = app.merge(gs_api::stream_route(stream_api_state));
        }
    }

    info!(
        version = VERSION,
        commit = GIT_COMMIT,
        roles = ?roles,
        http_addr = %http_addr,
        "starting geostream server"
    );

    let listener = tokio::net::TcpListener::bind(http_addr)
        .await
        .with_context(|| format!("failed to bind HTTP listener to {http_addr}"))?;

    info!(addr = %http_addr, "listening for incoming HTTP requests");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("server error while running HTTP listener")?;

    if let Some(dir) = ephemeral_key_dir {
        let _ = std::fs::remove_dir_all(dir);
    }

    info!("geostream server shutdown completed");
    Ok(())
}

/// Builds the readiness probe of the configured dependencies (docs/16 §4.1).
///
/// `/health/ready` answers 503 whenever PostgreSQL does not answer within the documented
/// timeout; the ephemeral store and the bus are reported when they are configured.
fn readiness_probe(database_url: Option<&str>) -> gs_api::ReadinessProbe {
    let database_url = database_url.map(str::to_string);
    match database_url {
        Some(url) => gs_api::ReadinessProbe::new(move || {
            let url = url.clone();
            async move {
                let mut checked = Vec::new();
                match tokio::time::timeout(gs_api::READINESS_TIMEOUT, async {
                    let pool = gs_api::migrate::connect_pool(&url).await?;
                    let connection = pool.acquire().await?;
                    Ok::<_, gs_api::migrate::StoreError>(connection)
                })
                .await
                {
                    Ok(Ok(_connection)) => checked.push(gs_api::DependencyStatus {
                        name: "postgresql".to_string(),
                        ok: true,
                        detail: None,
                    }),
                    Ok(Err(error)) => checked.push(gs_api::DependencyStatus {
                        name: "postgresql".to_string(),
                        ok: false,
                        detail: Some(error.to_string()),
                    }),
                    Err(_elapsed) => checked.push(gs_api::DependencyStatus {
                        name: "postgresql".to_string(),
                        ok: false,
                        detail: Some("timeout".to_string()),
                    }),
                }
                checked
            }
        }),
        None => gs_api::ReadinessProbe::default(),
    }
}

/// Graceful shutdown listener for SIGINT and SIGTERM signals.
async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut stream) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            stream.recv().await;
        } else {
            std::future::pending::<()>().await;
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
