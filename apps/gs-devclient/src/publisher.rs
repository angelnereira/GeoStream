//! Stream publisher logic for gs-devclient.
#![forbid(unsafe_code)]

use anyhow::{Context, Result, bail};
use gs_domain::Subprotocol;
use gs_protocol::client_message::Body as ClientBody;
use gs_protocol::server_message::Body as ServerBody;
use gs_protocol::{ClientHello, PublishLocation};
use gs_testkit::client::TestStreamClient;
use std::time::Duration;

use crate::trajectory::TrajectoryGenerator;

pub struct PublisherConfig {
    pub ws_url: String,
    pub session_id: String,
    pub subprotocol: Subprotocol,
    pub interval_ms: u64,
    pub count: u64,
    pub batch_size: usize,
}

pub async fn run_publisher(
    config: PublisherConfig,
    mut trajectory: TrajectoryGenerator,
) -> Result<()> {
    tracing::info!(
        session = %config.session_id,
        subprotocol = config.subprotocol.as_str(),
        interval_ms = config.interval_ms,
        count = config.count,
        "Connecting publisher..."
    );

    let client = TestStreamClient::connect(&config.ws_url, config.subprotocol)
        .await
        .context("WebSocket connection failed")?;

    // Step 1: Handshake
    let hello_id = client.next_id().await;
    let hello_reply = client
        .send_with_reply(
            hello_id,
            ClientBody::Hello(ClientHello {
                protocol_minor: 0,
                sdk_name: "gs-devclient".to_string(),
                sdk_version: "0.1.0".to_string(),
                platform: 1,
                app_version: "1.0.0".to_string(),
                last_sequence: 0,
                client_time: None,
                core_version: String::new(),
            }),
            Duration::from_secs(5),
        )
        .await
        .context("ClientHello handshake failed")?;

    match hello_reply.body {
        Some(ServerBody::Hello(server_hello)) => {
            let server_time_sec = server_hello
                .server_time
                .as_ref()
                .map(|t| t.seconds)
                .unwrap_or(0);
            println!(
                "✅ Connected! Connection ID: {}, Server Time (epoch): {}, Sequence Floor: {}",
                server_hello.connection_id, server_time_sec, server_hello.sequence_floor
            );
        }
        Some(ServerBody::Error(err)) => {
            bail!("Server returned error on handshake: {err:?}");
        }
        other => {
            bail!("Unexpected response to ClientHello: {other:?}");
        }
    }

    // Step 2: Publication loop
    let mut sent_count = 0u64;
    let step_duration = Duration::from_millis(config.interval_ms);

    loop {
        if config.count > 0 && sent_count >= config.count {
            println!("🎯 Published requested {sent_count} samples. Finishing.");
            break;
        }

        let mut samples = Vec::with_capacity(config.batch_size);
        for _ in 0..config.batch_size {
            samples.push(trajectory.next_sample());
        }

        let first_seq = samples[0].sequence;
        let (lat, lon, spd, hdg) = if let Some(pos) = samples[0].position.as_ref() {
            (
                pos.latitude,
                pos.longitude,
                pos.speed_mps.unwrap_or(0.0),
                pos.heading_deg.unwrap_or(0.0),
            )
        } else {
            (0.0, 0.0, 0.0, 0.0)
        };
        let pub_id = client.next_id().await;

        let reply = client
            .send_with_reply(
                pub_id,
                ClientBody::PublishLocation(PublishLocation {
                    session_id: config.session_id.clone(),
                    samples,
                }),
                Duration::from_secs(5),
            )
            .await;

        match reply {
            Ok(server_msg) => match server_msg.body {
                Some(ServerBody::PublishAck(ack)) => {
                    let res_str = ack
                        .results
                        .iter()
                        .map(|r| format!("{:?}", r.status()))
                        .collect::<Vec<_>>()
                        .join(", ");

                    println!(
                        "[PUB] Seq #{:<4} | Lat: {:>10.6}, Lon: {:>11.6} | Spd: {:>4.1} m/s | Hdg: {:>5.1}° | Results: [{res_str}]",
                        first_seq, lat, lon, spd, hdg
                    );
                    sent_count += 1;
                }
                Some(ServerBody::Error(err)) => {
                    eprintln!("⚠️ Server error on publish: {err:?}");
                    if err.code == 6 {
                        // Rate limited
                        let retry_ms = if err.retry_after_ms > 0 {
                            err.retry_after_ms
                        } else {
                            1000
                        };
                        tokio::time::sleep(Duration::from_millis(retry_ms as u64)).await;
                    }
                }
                other => {
                    eprintln!("⚠️ Unexpected reply: {other:?}");
                }
            },
            Err(e) => {
                eprintln!("❌ Failed to receive publish ack: {e}");
                break;
            }
        }

        tokio::time::sleep(step_duration).await;
    }

    let _ = client.close().await;
    Ok(())
}
