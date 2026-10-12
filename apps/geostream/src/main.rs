//! GeoStream server binary: serve, migrate, admin and version.
#![forbid(unsafe_code)]

use std::net::SocketAddr;
use std::path::PathBuf;
use std::str::FromStr;

use anyhow::{Context, Result, bail};
use axum::{Json, Router, routing::get};
use clap::{Args, Parser, Subcommand, ValueEnum};
use serde_json::json;
use tracing::info;

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
}

/// Arguments for `migrate` subcommand.
#[derive(Debug, Args)]
pub struct MigrateArgs {
    /// Show migration plan without applying changes.
    #[arg(long)]
    pub dry_run: bool,
}

/// Arguments for `admin` subcommand.
#[derive(Debug, Args)]
pub struct AdminArgs {
    /// Subcommand arguments passed to admin.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub args: Vec<String>,
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
            let dry_run_suffix = if args.dry_run { " (dry-run)" } else { "" };
            println!(
                "migrate{} is not implemented yet (scheduled for F2, task T-F2-01)",
                dry_run_suffix
            );
            Ok(())
        }
        Commands::Admin(_args) => {
            println!("admin is not implemented yet (scheduled for F2, task T-F2-03)");
            Ok(())
        }
        Commands::Serve(args) => run_serve(args).await,
    }
}

/// Run the server listener.
async fn run_serve(args: ServeArgs) -> Result<()> {
    // Initialize tracing subscriber if not already initialized
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .try_init();

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

    let app = router();
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

/// Build the Axum router for the HTTP listener.
pub fn router() -> Router {
    Router::new().route("/health/live", get(health_live_handler))
}

/// Handler for `/health/live` returning HTTP 200 OK.
async fn health_live_handler() -> Json<serde_json::Value> {
    Json(json!({
        "status": "ok"
    }))
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
