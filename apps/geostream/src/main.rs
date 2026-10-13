//! GeoStream server binary: serve, migrate, admin and version.
#![forbid(unsafe_code)]

pub mod admin;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::str::FromStr;

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand, ValueEnum};
use tracing::info;

use crate::admin::{AdminArgs, run_admin};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const GIT_COMMIT: &str = env!("GEOSTREAM_GIT_COMMIT");

/// Available server roles (FR-OPS-001).
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "lowercase")]
pub enum Role {
    /// REST control plane and public HTTP endpoints.
    Api,
    /// WebSocket location streaming endpoint.
    Stream,
    /// Asynchronous jobs and background workers.
    Worker,
}

impl FromStr for Role {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "api" => Ok(Self::Api),
            "stream" => Ok(Self::Stream),
            "worker" => Ok(Self::Worker),
            other => bail!("unknown role '{other}', expected one of: api, stream, worker"),
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
    /// Active roles (comma-separated or repeated: api, stream, worker). Defaults to all three.
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

/// Run the server listener.
async fn run_serve(args: ServeArgs) -> Result<()> {
    // Plain tracing subscriber. Wiring `gs_observability::init_from_env` (JSON logs with the
    // fields of docs/16 §4.3, GS_LOG_FORMAT, GS_LOG_UNSAFE_LOCATIONS) is blocked by
    // DR-20261012-04: `tools/check_crate_deps` does not allow `geostream` to depend on
    // `gs-observability`, which docs/04 §4.1 explicitly permits.
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .try_init();

    // Verify database schema version or auto-migrate (docs/11 §4)
    let database_url = std::env::var("GS_DATABASE_URL")
        .ok()
        .or_else(|| args.database_url.clone());
    if let Some(db_url) = database_url.clone() {
        let pool = gs_api::migrate::connect_pool(&db_url)
            .await
            .context("failed to connect to database on server startup")?;

        if args.auto_migrate {
            if args.env != "dev" {
                bail!(
                    "GS_AUTO_MIGRATE=true is only allowed in GS_ENV=dev (current env: {})",
                    args.env
                );
            }
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
    }

    let roles = args
        .roles
        .unwrap_or_else(|| vec![Role::Api, Role::Stream, Role::Worker]);
    info!(
        version = VERSION,
        commit = GIT_COMMIT,
        roles = ?roles,
        http_addr = %args.http_addr,
        "starting geostream server"
    );

    // Readiness checks the configured dependencies (docs/16 §4.1, FR-OPS-002).
    let probe = readiness_probe(database_url.as_deref());
    let app = gs_api::http_router_with_readiness(gs_api::DEFAULT_MAX_BODY_BYTES, probe);
    let listener = tokio::net::TcpListener::bind(args.http_addr)
        .await
        .with_context(|| format!("failed to bind HTTP listener to {}", args.http_addr))?;

    info!(addr = %args.http_addr, "listening for incoming HTTP requests");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("server error while running HTTP listener")?;

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
