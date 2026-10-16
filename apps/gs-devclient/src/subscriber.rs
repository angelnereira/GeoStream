//! Stream subscriber logic for gs-devclient.
#![forbid(unsafe_code)]

use anyhow::{Context, Result, bail};
use gs_domain::Subprotocol;
use gs_protocol::client_message::Body as ClientBody;
use gs_protocol::server_message::Body as ServerBody;
use gs_protocol::{ClientHello, Subscribe};
use gs_testkit::client::TestStreamClient;
use std::time::Duration;

pub struct SubscriberConfig {
    pub ws_url: String,
    pub session_id: String,
    pub subprotocol: Subprotocol,
    pub count: u64,
    pub json_format: bool,
}

pub async fn run_subscriber(config: SubscriberConfig) -> Result<()> {
    tracing::info!(
        session = %config.session_id,
        subprotocol = config.subprotocol.as_str(),
        "Connecting subscriber..."
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
                "✅ Connected! Connection ID: {}, Server Time (epoch): {}",
                server_hello.connection_id, server_time_sec
            );
        }
        Some(ServerBody::Error(err)) => {
            bail!("Server returned error on handshake: {err:?}");
        }
        other => {
            bail!("Unexpected response to ClientHello: {other:?}");
        }
    }

    // Step 2: Subscribe
    let sub_id = client.next_id().await;
    let sub_reply = client
        .send_with_reply(
            sub_id,
            ClientBody::Subscribe(Subscribe {
                session_id: config.session_id.clone(),
                skip_snapshot: false,
                replay_trace: false,
            }),
            Duration::from_secs(5),
        )
        .await
        .context("Subscribe request failed")?;

    match sub_reply.body {
        Some(ServerBody::SubscribeAck(ack)) => {
            println!(
                "📡 Subscribed to session {}! Members: {}, Initial Snapshot: {}",
                config.session_id,
                ack.members.len(),
                ack.snapshot.len()
            );
            for m in &ack.members {
                println!(
                    "   👤 Member: {} ({}) | Role: {:?} | Sharing: {} | Connectivity: {:?}",
                    m.member_id,
                    m.external_subject_id,
                    m.role(),
                    m.sharing,
                    m.connectivity()
                );
            }
            for s in &ack.snapshot {
                let pos = s.sample.as_ref().and_then(|sm| sm.position.as_ref());
                if let Some(p) = pos {
                    println!(
                        "   📍 Snapshot: {} | Lat: {:.6}, Lon: {:.6} | Seq: {}",
                        s.member_id,
                        p.latitude,
                        p.longitude,
                        s.sample.as_ref().map(|sm| sm.sequence).unwrap_or(0)
                    );
                }
            }
        }
        Some(ServerBody::Error(err)) => {
            bail!("Subscription rejected by server: {err:?}");
        }
        other => {
            bail!("Unexpected reply to Subscribe: {other:?}");
        }
    }

    // Step 3: Stream reception loop
    let mut received_count = 0u64;

    while config.count == 0 || received_count < config.count {
        match client.next_message(Duration::from_millis(500)).await {
            Ok(Some(msg)) => match msg.body {
                Some(ServerBody::LocationUpdate(update)) => {
                    received_count += 1;
                    if config.json_format {
                        let json = serde_json::to_string(&update).unwrap_or_default();
                        println!("{json}");
                    } else {
                        let sample = update.sample.as_ref();
                        let pos = sample.and_then(|s| s.position.as_ref());
                        let seq = sample.map(|s| s.sequence).unwrap_or(0);
                        let lat = pos.map(|p| p.latitude).unwrap_or(0.0);
                        let lon = pos.map(|p| p.longitude).unwrap_or(0.0);
                        let spd = pos.and_then(|p| p.speed_mps).unwrap_or(0.0);
                        let hdg = pos.and_then(|p| p.heading_deg).unwrap_or(0.0);

                        println!(
                            "[SUB #{:<3}] 📍 Member: {:<12} | Seq #{:<4} | Lat: {:>10.6}, Lon: {:>11.6} | Spd: {:>4.1} m/s | Hdg: {:>5.1}°",
                            received_count, update.member_id, seq, lat, lon, spd, hdg
                        );
                    }
                }
                Some(ServerBody::MemberStatus(status)) => {
                    println!(
                        "[SUB] 👤 Status change: {} ({}) -> {:?}",
                        status.member_id,
                        status.external_subject_id,
                        status.connectivity()
                    );
                }
                Some(ServerBody::SubscriptionEnded(ended)) => {
                    println!(
                        "[SUB] 🛑 Subscription ended for session {}: {:?}",
                        ended.session_id,
                        ended.reason()
                    );
                    break;
                }
                Some(ServerBody::SessionEvent(ev)) => {
                    println!("[SUB] ℹ️ Session event: {ev:?}");
                }
                _ => {}
            },
            Ok(None) => {
                // Timeout on this tick or connection closed, continue waiting unless closed
            }
            Err(e) => {
                eprintln!("❌ Error reading message: {e}");
                break;
            }
        }
    }

    let _ = client.close().await;
    Ok(())
}
