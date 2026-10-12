//! `geostream admin` subcommands (docs/16 §3, FR-ADM-001, FR-ADM-002).
//!
//! Output is intentionally plain and scriptable: `tenant create` and `app create` print the new
//! identifier, `key create` prints the credential exactly once and `policy` prints JSON.
#![forbid(unsafe_code)]

use std::path::PathBuf;

use anyhow::{Context, Result, anyhow, bail};
use clap::{Args, Subcommand};
use time::{Duration, OffsetDateTime, format_description::well_known::Rfc3339};

use gs_api::admin::{
    AdminError, create_api_key, create_application, create_tenant, generate_signing_key_pair,
    get_effective_policy, list_api_keys, list_applications, list_tenants, revoke_api_key,
    set_application_enabled, set_policy_values, set_tenant_enabled,
};
use gs_api::migrate::connect_pool;

/// Seconds of each accepted unit of `--expires-in`.
fn unit_seconds(unit: &str) -> Option<i64> {
    match unit {
        "m" => Some(60),
        "h" => Some(3_600),
        "d" => Some(86_400),
        _ => None,
    }
}

/// Parses `--expires-in` values such as `90d`, `24h` or `30m` (docs/16 §3).
fn parse_expires_in(raw: &str) -> Result<Duration> {
    let raw = raw.trim();
    let boundary = raw.find(|c: char| !c.is_ascii_digit()).ok_or_else(|| {
        anyhow!("invalid --expires-in '{raw}': expected <number><unit> (m, h or d)")
    })?;
    let (number, unit) = raw.split_at(boundary);
    let number: i64 = number
        .parse()
        .with_context(|| format!("invalid number in --expires-in '{raw}'"))?;
    let seconds = unit_seconds(unit).ok_or_else(|| {
        anyhow!("invalid unit '{unit}' in --expires-in '{raw}': expected m, h or d")
    })?;
    if number <= 0 {
        bail!("--expires-in must be greater than zero (got '{raw}')");
    }
    let total = number
        .checked_mul(seconds)
        .ok_or_else(|| anyhow!("--expires-in value '{raw}' is too large"))?;
    Ok(Duration::seconds(total))
}

/// Formats an instant as RFC 3339, or `-` when absent (docs/00 §4.5).
fn format_timestamp(instant: Option<OffsetDateTime>) -> String {
    instant.map_or_else(|| "-".to_string(), format_instant)
}

/// Formats an instant as RFC 3339 (docs/00 §4.5).
fn format_instant(instant: OffsetDateTime) -> String {
    instant
        .format(&Rfc3339)
        .unwrap_or_else(|_| instant.to_string())
}

/// Prints a JSON value as pretty JSON.
fn print_json(value: &serde_json::Value) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

/// Arguments of `geostream admin`.
#[derive(Debug, Args)]
pub struct AdminArgs {
    /// Database connection URL (defaults to GS_DATABASE_URL).
    #[arg(long, global = true, env = "GS_DATABASE_URL")]
    pub database_url: Option<String>,

    /// Administrative area (docs/16 §3).
    #[command(subcommand)]
    pub command: AdminCommands,
}

/// Administrative areas (docs/16 §3).
#[derive(Debug, Subcommand)]
pub enum AdminCommands {
    /// Tenant management.
    Tenant(TenantOptions),
    /// Application management.
    App(AppOptions),
    /// API key management.
    Key(KeyOptions),
    /// Tenant policy management.
    Policy(PolicyOptions),
    /// Ed25519 signing key pairs for device JWTs.
    SigningKey(SigningKeyOptions),
    /// Session handling for incident response.
    Session(SessionOptions),
}

/// `admin tenant` subcommands (docs/16 §3).
#[derive(Debug, Args)]
pub struct TenantOptions {
    #[command(subcommand)]
    pub command: TenantCommands,
}

/// Tenant actions.
#[derive(Debug, Subcommand)]
pub enum TenantCommands {
    /// Create a tenant and print its id.
    Create(TenantCreateArgs),
    /// List tenants.
    List,
    /// Disable a tenant, blocking REST and stream access.
    Disable(TenantRefArgs),
    /// Re-enable a disabled tenant.
    Enable(TenantRefArgs),
}

/// `admin tenant create` (docs/16 §3).
#[derive(Debug, Args)]
pub struct TenantCreateArgs {
    /// Tenant name.
    #[arg(long)]
    pub name: String,
}

/// Reference to a tenant by id.
#[derive(Debug, Args)]
pub struct TenantRefArgs {
    /// Tenant id (`ten_…`).
    #[arg(long)]
    pub tenant: String,
}

/// `admin app` subcommands (docs/16 §3).
#[derive(Debug, Args)]
pub struct AppOptions {
    #[command(subcommand)]
    pub command: AppCommands,
}

/// Application actions.
#[derive(Debug, Subcommand)]
pub enum AppCommands {
    /// Create an application and print its id.
    Create(AppCreateArgs),
    /// List the applications of a tenant.
    List(AppListArgs),
    /// Disable an application.
    Disable(AppRefArgs),
    /// Re-enable a disabled application.
    Enable(AppRefArgs),
}

/// `admin app create` (docs/16 §3).
#[derive(Debug, Args)]
pub struct AppCreateArgs {
    /// Owning tenant id (`ten_…`).
    #[arg(long)]
    pub tenant: String,
    /// Application name.
    #[arg(long)]
    pub name: String,
    /// Allowed device scopes, comma-separated (defaults to docs/11 §2).
    #[arg(long, value_delimiter = ',')]
    pub device_scopes: Option<Vec<String>>,
    /// Allowed CORS origins, comma-separated.
    #[arg(long, value_delimiter = ',')]
    pub origins: Option<Vec<String>>,
}

/// `admin app list` (docs/16 §3).
#[derive(Debug, Args)]
pub struct AppListArgs {
    /// Tenant id (`ten_…`).
    #[arg(long)]
    pub tenant: String,
}

/// Reference to an application by id.
#[derive(Debug, Args)]
pub struct AppRefArgs {
    /// Application id (`app_…`).
    #[arg(long)]
    pub app: String,
}

/// `admin key` subcommands (docs/16 §3).
#[derive(Debug, Args)]
pub struct KeyOptions {
    #[command(subcommand)]
    pub command: KeyCommands,
}

/// API key actions.
#[derive(Debug, Subcommand)]
pub enum KeyCommands {
    /// Create an API key: prints the `gsk_…` credential once.
    Create(KeyCreateArgs),
    /// List the API keys of an application (never shows secrets).
    List(KeyListArgs),
    /// Revoke an API key.
    Revoke(KeyRefArgs),
}

/// `admin key create` (docs/16 §3).
#[derive(Debug, Args)]
pub struct KeyCreateArgs {
    /// Application id (`app_…`).
    #[arg(long)]
    pub app: String,
    /// Scopes, comma-separated (defaults to all of docs/06 §3.4 except `locations:read`).
    #[arg(long, value_delimiter = ',')]
    pub scopes: Option<Vec<String>>,
    /// Human readable label.
    #[arg(long)]
    pub label: Option<String>,
    /// Expiry as `<number><unit>` with unit `m`, `h` or `d` (for example `90d`).
    #[arg(long)]
    pub expires_in: Option<String>,
}

/// `admin key list` (docs/16 §3).
#[derive(Debug, Args)]
pub struct KeyListArgs {
    /// Application id (`app_…`).
    #[arg(long)]
    pub app: String,
}

/// Reference to an API key by id.
#[derive(Debug, Args)]
pub struct KeyRefArgs {
    /// API key id (`key_…`).
    #[arg(long)]
    pub key: String,
}

/// `admin policy` subcommands (docs/16 §3).
#[derive(Debug, Args)]
pub struct PolicyOptions {
    #[command(subcommand)]
    pub command: PolicyCommands,
}

/// Policy actions.
#[derive(Debug, Subcommand)]
pub enum PolicyCommands {
    /// Show the effective tenant policy (defaults plus stored overrides).
    Get(PolicyGetArgs),
    /// Set tenant policy overrides given as `clave=valor` (docs/17 §4).
    Set(PolicySetArgs),
}

/// `admin policy get` (docs/16 §3).
#[derive(Debug, Args)]
pub struct PolicyGetArgs {
    /// Tenant id (`ten_…`).
    #[arg(long)]
    pub tenant: String,
}

/// `admin policy set` (docs/16 §3).
#[derive(Debug, Args)]
pub struct PolicySetArgs {
    /// Tenant id (`ten_…`).
    #[arg(long)]
    pub tenant: String,
    /// Policy assignments in the form `clave=valor`.
    #[arg(required = true, num_args = 1..)]
    pub assignments: Vec<String>,
}

/// `admin signing-key` subcommands (docs/06 §4.4).
#[derive(Debug, Args)]
pub struct SigningKeyOptions {
    #[command(subcommand)]
    pub command: SigningKeyCommands,
}

/// Signing key actions.
#[derive(Debug, Subcommand)]
pub enum SigningKeyCommands {
    /// Generate an Ed25519 signing key pair for device JWTs.
    Generate(SigningKeyGenerateArgs),
}

/// `admin signing-key generate` (docs/06 §4.4).
#[derive(Debug, Args)]
pub struct SigningKeyGenerateArgs {
    /// Key id to generate (`^[a-z0-9-]{1,64}$`).
    #[arg(long)]
    pub kid: String,
    /// Output directory (`GS_JWT_KEYS_DIR`).
    #[arg(long)]
    pub out: PathBuf,
}

/// `admin session` subcommands (docs/16 §3).
#[derive(Debug, Args)]
pub struct SessionOptions {
    #[command(subcommand)]
    pub command: SessionCommands,
}

/// Session actions.
#[derive(Debug, Subcommand)]
pub enum SessionCommands {
    /// Close a session for incident handling.
    Close(SessionCloseArgs),
}

/// `admin session close` (docs/16 §3).
#[derive(Debug, Args)]
pub struct SessionCloseArgs {
    /// Session id (`ses_…`).
    #[arg(long)]
    pub session: String,
    /// Closure reason recorded in the audit trail (docs/11 §2: `ADMIN`).
    #[arg(long, default_value = "ADMIN")]
    pub reason: String,
}

/// Runs an administrative command.
pub async fn run_admin(args: AdminArgs) -> Result<()> {
    let AdminArgs {
        database_url,
        command,
    } = args;

    match command {
        AdminCommands::SigningKey(options) => run_signing_key(options),
        AdminCommands::Session(options) => run_session_close(options).await,
        command => run_with_database(database_url, command).await,
    }
}

/// Runs the commands that only touch the filesystem.
fn run_signing_key(options: SigningKeyOptions) -> Result<()> {
    let SigningKeyCommands::Generate(args) = options.command;
    let generated = generate_signing_key_pair(&args.kid, &args.out).map_err(|e| {
        anyhow!(match e {
            AdminError::SigningKey(inner) => inner.to_string(),
            other => other.to_string(),
        })
    })?;
    println!(
        "Generated signing key pair '{}': {} (0600) and {}",
        generated.kid,
        generated.private_key_path.display(),
        generated.public_key_path.display()
    );
    Ok(())
}

/// Reports the closure of a session: implemented in F3 (T-F3-03).
async fn run_session_close(options: SessionOptions) -> Result<()> {
    let SessionCommands::Close(args) = options.command;
    bail!(
        "session close is not implemented yet (scheduled for F3, task T-F3-03): session {}, reason {}",
        args.session,
        args.reason
    )
}

/// Runs the commands that need a database connection.
async fn run_with_database(database_url: Option<String>, command: AdminCommands) -> Result<()> {
    let database_url = database_url
        .context("missing database URL: provide --database-url or set GS_DATABASE_URL")?;
    let pool = connect_pool(&database_url)
        .await
        .context("failed to connect to database")?;

    match command {
        AdminCommands::Tenant(options) => run_tenant(&pool, options).await,
        AdminCommands::App(options) => run_app(&pool, options).await,
        AdminCommands::Key(options) => run_key(&pool, options).await,
        AdminCommands::Policy(options) => run_policy(&pool, options).await,
        // INVARIANT: `run_admin` dispatches `SigningKey` and `Session` before calling this
        // function because neither of them needs a database connection.
        AdminCommands::SigningKey(_) | AdminCommands::Session(_) => {
            bail!("this command does not use the database")
        }
    }
}

/// Runs the tenant subcommands (docs/16 §3).
async fn run_tenant(pool: &gs_api::migrate::PgPool, options: TenantOptions) -> Result<()> {
    match options.command {
        TenantCommands::Create(args) => {
            let tenant = create_tenant(pool, &args.name)
                .await
                .map_err(map_admin_error)?;
            println!("{}", tenant.id);
        }
        TenantCommands::List => {
            let tenants = list_tenants(pool).await.map_err(map_admin_error)?;
            for tenant in tenants {
                println!(
                    "{}\t{}\t{}\t{}",
                    tenant.id,
                    tenant.status,
                    tenant.name,
                    format_instant(tenant.created_at)
                );
            }
        }
        TenantCommands::Disable(args) => {
            let tenant = set_tenant_enabled(pool, &args.tenant, false)
                .await
                .map_err(map_admin_error)?;
            println!(
                "{} ({}) disabled at {}",
                tenant.id,
                tenant.name,
                format_instant(tenant.updated_at)
            );
        }
        TenantCommands::Enable(args) => {
            let tenant = set_tenant_enabled(pool, &args.tenant, true)
                .await
                .map_err(map_admin_error)?;
            println!(
                "{} ({}) enabled at {}",
                tenant.id,
                tenant.name,
                format_instant(tenant.updated_at)
            );
        }
    }
    Ok(())
}

/// Runs the application subcommands (docs/16 §3).
async fn run_app(pool: &gs_api::migrate::PgPool, options: AppOptions) -> Result<()> {
    match options.command {
        AppCommands::Create(args) => {
            let application = create_application(
                pool,
                &args.tenant,
                &args.name,
                args.device_scopes,
                args.origins,
            )
            .await
            .map_err(map_admin_error)?;
            println!("{}", application.id);
        }
        AppCommands::List(args) => {
            let applications = list_applications(pool, &args.tenant)
                .await
                .map_err(map_admin_error)?;
            for application in applications {
                println!(
                    "{}\t{}\t{}\t{}\t{}",
                    application.id,
                    application.status,
                    application.name,
                    application.allowed_device_scopes.join(","),
                    format_instant(application.created_at)
                );
            }
        }
        AppCommands::Disable(args) => {
            let application = set_application_enabled(pool, &args.app, false)
                .await
                .map_err(map_admin_error)?;
            println!(
                "{} (tenant {}) disabled at {}",
                application.id,
                application.tenant_id,
                format_instant(application.updated_at)
            );
        }
        AppCommands::Enable(args) => {
            let application = set_application_enabled(pool, &args.app, true)
                .await
                .map_err(map_admin_error)?;
            println!(
                "{} (tenant {}) enabled at {}",
                application.id,
                application.tenant_id,
                format_instant(application.updated_at)
            );
        }
    }
    Ok(())
}

/// Runs the API key subcommands (docs/16 §3).
async fn run_key(pool: &gs_api::migrate::PgPool, options: KeyOptions) -> Result<()> {
    match options.command {
        KeyCommands::Create(args) => {
            let expires_in = match args.expires_in.as_deref() {
                Some(raw) => Some(parse_expires_in(raw)?),
                None => None,
            };
            let created = create_api_key(
                pool,
                &args.app,
                args.scopes,
                args.label.as_deref(),
                expires_in,
            )
            .await
            .map_err(map_admin_error)?;
            // The secret exists only here: it is never stored, logged or shown again.
            println!("{}", created.credential.as_str());
        }
        KeyCommands::List(args) => {
            let keys = list_api_keys(pool, &args.app)
                .await
                .map_err(map_admin_error)?;
            for key in keys {
                println!(
                    "{}\t{}\t{}\t{}\t{}\t{}",
                    key.id,
                    key.status,
                    key.label.as_deref().unwrap_or("-"),
                    key.scopes.join(","),
                    format_timestamp(key.expires_at),
                    format_timestamp(key.last_used_at),
                );
            }
        }
        KeyCommands::Revoke(args) => {
            let key = revoke_api_key(pool, &args.key)
                .await
                .map_err(map_admin_error)?;
            println!(
                "{} ({}) revoked",
                key.id,
                key.label.as_deref().unwrap_or("-")
            );
        }
    }
    Ok(())
}

/// Runs the policy subcommands (docs/16 §3, docs/17 §4).
async fn run_policy(pool: &gs_api::migrate::PgPool, options: PolicyOptions) -> Result<()> {
    match options.command {
        PolicyCommands::Get(args) => {
            let policy = get_effective_policy(pool, &args.tenant)
                .await
                .map_err(map_admin_error)?;
            print_json(&serde_json::to_value(&policy)?)?;
        }
        PolicyCommands::Set(args) => {
            let policy = set_policy_values(pool, &args.tenant, &args.assignments)
                .await
                .map_err(map_admin_error)?;
            println!("Updated policy of tenant {}", args.tenant);
            print_json(&serde_json::to_value(&policy)?)?;
        }
    }
    Ok(())
}

/// Converts an administrative error into an anyhow error.
fn map_admin_error(error: AdminError) -> anyhow::Error {
    anyhow!(error.to_string())
}
