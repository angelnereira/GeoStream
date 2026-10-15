//! Development CLI: connect, subscribe and publish synthetic trajectories.
#![forbid(unsafe_code)]

use std::str::FromStr;
use anyhow::{Result, bail};
use clap::{Parser, Subcommand};
use gs_devclient::publisher::{PublisherConfig, run_publisher};
use gs_devclient::subscriber::{SubscriberConfig, run_subscriber};
use gs_devclient::trajectory::{TrajectoryGenerator, TrajectoryKind};
use gs_devclient::{demo, rest};
use gs_domain::Subprotocol;

#[derive(Parser, Debug)]
#[command(
    name = "gs-devclient",
    version,
    about = "Development CLI: connect, subscribe and publish synthetic trajectories",
    long_about = None
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Publish a synthetic trajectory to a stream session
    Publish {
        /// Base REST or stream URL (e.g. http://127.0.0.1:8080 or ws://127.0.0.1:8080/v1/stream)
        #[arg(long, default_value = "http://127.0.0.1:8080")]
        url: String,

        /// Session ID to publish to
        #[arg(short, long)]
        session_id: String,

        /// Bearer device token (used to get stream ticket and enable sharing)
        #[arg(short, long)]
        token: Option<String>,

        /// Stream ticket (gst_...) if already obtained
        #[arg(long)]
        ticket: Option<String>,

        /// Subprotocol (proto or json)
        #[arg(long, default_value = "proto")]
        subprotocol: String,

        /// Synthetic trajectory type (circle, linear, sine)
        #[arg(long, default_value = "circle")]
        trajectory: String,

        /// Center latitude
        #[arg(long, default_value_t = -12.0464)]
        lat: f64,

        /// Center longitude
        #[arg(long, default_value_t = -77.0428)]
        lon: f64,

        /// Speed in m/s
        #[arg(long, default_value_t = 10.0)]
        speed: f64,

        /// Trajectory radius in meters (for circular trajectories)
        #[arg(long, default_value_t = 400.0)]
        radius: f64,

        /// Starting sequence number
        #[arg(long, default_value_t = 1)]
        start_seq: u64,

        /// Interval between samples in milliseconds
        #[arg(long, default_value_t = 1000)]
        interval_ms: u64,

        /// Number of samples to publish (0 for continuous)
        #[arg(long, default_value_t = 0)]
        count: u64,

        /// Ensure sharing is enabled via REST before publishing
        #[arg(long)]
        ensure_sharing: bool,
    },

    /// Subscribe to a stream session and display incoming location updates
    Subscribe {
        /// Base REST or stream URL
        #[arg(long, default_value = "http://127.0.0.1:8080")]
        url: String,

        /// Session ID to subscribe to
        #[arg(short, long)]
        session_id: String,

        /// Bearer token (device JWT or application API key)
        #[arg(short, long)]
        token: Option<String>,

        /// Stream ticket (gst_...) if already obtained
        #[arg(long)]
        ticket: Option<String>,

        /// Subprotocol (proto or json)
        #[arg(long, default_value = "proto")]
        subprotocol: String,

        /// Output format as JSON
        #[arg(long)]
        json: bool,

        /// Number of updates to receive before exiting (0 for continuous)
        #[arg(long, default_value_t = 0)]
        count: u64,
    },

    /// Fetch latest location snapshot via REST
    Snapshot {
        /// Base REST URL
        #[arg(long, default_value = "http://127.0.0.1:8080")]
        url: String,

        /// Session ID
        #[arg(short, long)]
        session_id: String,

        /// Bearer token (device JWT or application API key)
        #[arg(short, long)]
        token: String,
    },

    /// Run the M1 demonstration (2 publishers and 1 reader in real-time)
    Demo {
        /// Duration of the demo in seconds
        #[arg(long, default_value_t = 5)]
        duration: u64,

        /// Subprotocol (proto or json)
        #[arg(long, default_value = "proto")]
        subprotocol: String,

        /// Quiet output (only summary)
        #[arg(long)]
        quiet: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Publish {
            url,
            session_id,
            token,
            ticket,
            subprotocol,
            trajectory,
            lat,
            lon,
            speed,
            radius,
            start_seq,
            interval_ms,
            count,
            ensure_sharing,
        } => {
            let sub = parse_subprotocol(&subprotocol)?;
            let traj_kind =
                TrajectoryKind::from_str(&trajectory).map_err(|e| anyhow::anyhow!("{e}"))?;

            let ws_url = resolve_ws_url(&url, token.as_deref(), ticket.as_deref()).await?;

            if ensure_sharing {
                if let Some(tok) = token.as_deref() {
                    println!("🔄 Enabling location sharing for session {session_id}...");
                    rest::set_sharing(&url, &session_id, tok, true).await?;
                    println!("✅ Location sharing enabled.");
                } else {
                    eprintln!("⚠️ Cannot ensure sharing without a bearer token.");
                }
            }

            let generator = TrajectoryGenerator::new(traj_kind, lat, lon, speed, interval_ms)
                .with_radius(radius)
                .with_start_seq(start_seq);

            let config = PublisherConfig {
                ws_url,
                session_id,
                subprotocol: sub,
                interval_ms,
                count,
                batch_size: 1,
            };

            run_publisher(config, generator).await?;
        }

        Commands::Subscribe {
            url,
            session_id,
            token,
            ticket,
            subprotocol,
            json,
            count,
        } => {
            let sub = parse_subprotocol(&subprotocol)?;
            let ws_url = resolve_ws_url(&url, token.as_deref(), ticket.as_deref()).await?;

            let config = SubscriberConfig {
                ws_url,
                session_id,
                subprotocol: sub,
                count,
                json_format: json,
            };

            run_subscriber(config).await?;
        }

        Commands::Snapshot {
            url,
            session_id,
            token,
        } => {
            let snap = rest::get_snapshot(&url, &session_id, &token).await?;
            println!("{}", serde_json::to_string_pretty(&snap)?);
        }

        Commands::Demo {
            duration,
            subprotocol,
            quiet,
        } => {
            let sub = parse_subprotocol(&subprotocol)?;
            let demo_config = demo::DemoConfig {
                duration_secs: duration,
                subprotocol: sub,
                quiet,
            };
            demo::run_demo(demo_config).await?;
        }
    }

    Ok(())
}

fn parse_subprotocol(s: &str) -> Result<Subprotocol> {
    match s.to_lowercase().as_str() {
        "proto" | "gs.v1.proto" => Ok(Subprotocol::Proto),
        "json" | "gs.v1.json" => Ok(Subprotocol::Json),
        other => bail!("Invalid subprotocol '{other}'. Allowed: 'proto' or 'json'"),
    }
}

async fn resolve_ws_url(url: &str, token: Option<&str>, ticket: Option<&str>) -> Result<String> {
    if let Some(t) = ticket {
        let base = if url.starts_with("ws://") || url.starts_with("wss://") {
            url.to_string()
        } else {
            let trimmed = url.trim_end_matches('/');
            format!("{trimmed}/v1/stream")
                .replace("http://", "ws://")
                .replace("https://", "wss://")
        };
        let separator = if base.contains('?') { "&" } else { "?" };
        return Ok(format!("{base}{separator}ticket={t}"));
    }

    if let Some(tok) = token {
        let ticket_resp = rest::request_stream_ticket(url, tok).await?;
        return Ok(ticket_resp.stream_url);
    }

    // Direct url given with ticket already embedded
    if url.contains("ticket=") {
        return Ok(url.to_string());
    }

    bail!("Either --ticket, --token, or a WebSocket URL containing '?ticket=' must be provided");
}
