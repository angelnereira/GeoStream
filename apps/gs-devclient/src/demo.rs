//! Multi-participant live demonstration ("dos publicadores y un lector")
//! satisfying Milestone M1 exit criteria (docs/21_ROADMAP.md).
#![forbid(unsafe_code)]

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use gs_domain::Subprotocol;
use gs_protocol::client_message::Body as ClientBody;
use gs_protocol::server_message::Body as ServerBody;
use gs_protocol::{ClientHello, MotionActivity, PublishLocation, Subscribe};
use gs_testkit::client::TestStreamClient;
use gs_testkit::server::TestServer;
use reqwest::Client;
use serde_json::json;
use tokio::time::sleep;

use crate::rest::request_stream_ticket;
use crate::trajectory::{TrajectoryGenerator, TrajectoryKind};

/// Configuration options for the demo.
#[derive(Debug, Clone)]
pub struct DemoConfig {
    pub duration_secs: u64,
    pub subprotocol: Subprotocol,
    pub quiet: bool,
}

impl Default for DemoConfig {
    fn default() -> Self {
        Self {
            duration_secs: 5,
            subprotocol: Subprotocol::Proto,
            quiet: false,
        }
    }
}

/// Statistics collected during demo execution.
#[allow(dead_code)]
#[derive(Debug, Default)]
pub struct DemoStats {
    pub alpha_sent: u64,
    pub beta_sent: u64,
    pub dispatcher_received: u64,
    pub publishers_observed: HashSet<String>,
}

/// Runs the multi-participant demo: two publishers and one live reader.
pub async fn run_demo(config: DemoConfig) -> Result<DemoStats> {
    if !config.quiet {
        println!("🚀 ===========================================================");
        println!("   GeoStream M1 Demo: 2 Publicadores y 1 Lector en Vivo");
        println!("   Subprotocolo: {}", config.subprotocol.as_str());
        println!("   Duración: {} segundos", config.duration_secs);
        println!("===========================================================");
    }

    // 1. Start isolated in-process server
    if !config.quiet {
        println!("🔧 [1/5] Inicializando servidor de prueba y PostgreSQL...");
    }
    let server = TestServer::start(&BTreeMap::new())
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    let http_client = Client::builder().build()?;

    // 2. Provision Tenant and Application
    if !config.quiet {
        println!("🏢 [2/5] Aprovisionando Tenant 'Logistics Corp' y Application...");
    }
    let run_id = uuid::Uuid::new_v4().to_string();
    let tenant =
        gs_service::admin::create_tenant(&server.pool, &format!("Logistics Corp {}", &run_id[..6]))
            .await
            .map_err(|e| anyhow::anyhow!("Create tenant error: {e}"))?;

    let app = gs_service::admin::create_application(
        &server.pool,
        &tenant.id.to_string(),
        &format!("Fleet Tracker {}", &run_id[..6]),
        None,
        None,
    )
    .await
    .map_err(|e| anyhow::anyhow!("Create app error: {e}"))?;

    let app_scopes: Vec<String> = gs_auth::scopes::APPLICATION_SCOPES
        .iter()
        .map(|s| s.to_string())
        .collect();

    let api_key = gs_service::admin::create_api_key(
        &server.pool,
        &app.id.to_string(),
        Some(app_scopes),
        Some("demo-key"),
        None,
    )
    .await
    .map_err(|e| anyhow::anyhow!("Create api key error: {e}"))?;

    let app_token = api_key.credential.to_string();

    // 3. Create Session via REST
    if !config.quiet {
        println!("📦 [3/5] Creando sesión de despacho y miembros...");
    }
    let session_url = format!("{}/v1/sessions", server.http_base);
    let session_resp = http_client
        .post(&session_url)
        .bearer_auth(&app_token)
        .json(&json!({
            "mode": "MANY_TO_MANY",
            "metadata": {
                "title": "M1 Dispatch Center"
            }
        }))
        .send()
        .await
        .context("Failed to create session via REST")?;

    if !session_resp.status().is_success() {
        let err = session_resp.text().await.unwrap_or_default();
        bail!("Failed to create session: {err}");
    }
    let session_json: serde_json::Value = session_resp.json().await?;
    let session_id = session_json["id"]
        .as_str()
        .context("Missing session id")?
        .to_string();

    // 4. Provision members: Courier Alpha, Courier Beta, Live Dispatcher
    let add_member = |ext_id: &str, can_publish: bool| {
        let s_id = session_id.clone();
        let a_tok = app_token.clone();
        let base = server.http_base.clone();
        let client = http_client.clone();
        let ext_id_owned = ext_id.to_string();

        async move {
            let url = format!("{base}/v1/sessions/{s_id}/members");
            let resp = client
                .post(&url)
                .bearer_auth(&a_tok)
                .json(&json!({
                    "external_subject_id": ext_id_owned,
                    "role": "MEMBER",
                    "capabilities": {
                        "read": true,
                        "publish": can_publish,
                        "invite": false,
                        "manage": false
                    },
                }))
                .send()
                .await?;
            if !resp.status().is_success() {
                let text = resp.text().await.unwrap_or_default();
                bail!("Failed to add member {ext_id_owned}: {text}");
            }
            let val: serde_json::Value = resp.json().await?;
            let mem_id = val["id"].as_str().context("missing mem id")?.to_string();
            Ok::<String, anyhow::Error>(mem_id)
        }
    };

    let mem_alpha_id = add_member("courier-alpha", true).await?;
    let mem_beta_id = add_member("courier-beta", true).await?;
    let _mem_disp_id = add_member("dispatcher", false).await?;

    // Mint device tokens
    let dev_scopes = Some(vec![
        "stream:connect".to_string(),
        "sharing:write".to_string(),
    ]);

    let token_alpha = server
        .mint_token(
            &app_token,
            "courier-alpha",
            "00000000-0000-4000-8000-000000000001",
            "ANDROID",
            dev_scopes.clone(),
            Some(3600),
        )
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    let token_beta = server
        .mint_token(
            &app_token,
            "courier-beta",
            "00000000-0000-4000-8000-000000000002",
            "IOS",
            dev_scopes.clone(),
            Some(3600),
        )
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    let token_disp = server
        .mint_token(
            &app_token,
            "dispatcher",
            "00000000-0000-4000-8000-000000000003",
            "WEB",
            Some(vec!["stream:connect".to_string()]),
            Some(3600),
        )
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    // Enable sharing for Alpha and Beta
    crate::rest::set_sharing(&server.http_base, &session_id, &token_alpha, true)
        .await
        .context("Failed to enable sharing for Courier Alpha")?;
    crate::rest::set_sharing(&server.http_base, &session_id, &token_beta, true)
        .await
        .context("Failed to enable sharing for Courier Beta")?;

    // 5. Connect clients and run streaming demo
    if !config.quiet {
        println!("📡 [4/5] Conectando 1 Lector y 2 Publicadores WebSocket...");
    }

    // Connect Dispatcher (Subscriber)
    let ticket_disp = request_stream_ticket(&server.http_base, &token_disp).await?;
    let disp_ws_url = format!("{}/v1/stream?ticket={}", server.ws_base, ticket_disp.ticket);
    let sub_client = TestStreamClient::connect(&disp_ws_url, config.subprotocol).await?;
    let h_id = sub_client.next_id().await;
    sub_client
        .send_with_reply(
            h_id,
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
        .await?;

    let sub_req_id = sub_client.next_id().await;
    let sub_ack = sub_client
        .send_with_reply(
            sub_req_id,
            ClientBody::Subscribe(Subscribe {
                session_id: session_id.clone(),
                skip_snapshot: false,
                replay_trace: false,
            }),
            Duration::from_secs(5),
        )
        .await?;

    if !config.quiet
        && let Some(ServerBody::SubscribeAck(ack)) = sub_ack.body
    {
        println!(
            "   ✅ Lector suscrito a sesión '{}' ({} miembros activos)",
            session_id,
            ack.members.len()
        );
    }

    // Connect Publisher Alpha
    let ticket_alpha = request_stream_ticket(&server.http_base, &token_alpha).await?;
    let alpha_ws_url = format!(
        "{}/v1/stream?ticket={}",
        server.ws_base, ticket_alpha.ticket
    );
    let pub_alpha = TestStreamClient::connect(&alpha_ws_url, config.subprotocol).await?;
    let h_id = pub_alpha.next_id().await;
    pub_alpha
        .send_with_reply(
            h_id,
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
        .await?;

    // Connect Publisher Beta
    let ticket_beta = request_stream_ticket(&server.http_base, &token_beta).await?;
    let beta_ws_url = format!("{}/v1/stream?ticket={}", server.ws_base, ticket_beta.ticket);
    let pub_beta = TestStreamClient::connect(&beta_ws_url, config.subprotocol).await?;
    let h_id = pub_beta.next_id().await;
    pub_beta
        .send_with_reply(
            h_id,
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
        .await?;

    if !config.quiet {
        println!(
            "🚚 [5/5] Transmitiendo coordenadas en tiempo real durante {}s...",
            config.duration_secs
        );
    }

    let running = Arc::new(AtomicBool::new(true));
    let alpha_sent = Arc::new(AtomicU64::new(0));
    let beta_sent = Arc::new(AtomicU64::new(0));
    let disp_received = Arc::new(AtomicU64::new(0));
    let publishers_seen = Arc::new(tokio::sync::Mutex::new(HashSet::new()));

    // Task: Courier Alpha (Circular Path around Lima Plaza Mayor)
    let r_alpha = Arc::clone(&running);
    let s_alpha = Arc::clone(&alpha_sent);
    let sid_alpha = session_id.clone();
    let alpha_task = tokio::spawn(async move {
        let mut traj =
            TrajectoryGenerator::new(TrajectoryKind::Circle, -12.0464, -77.0428, 12.0, 500)
                .with_radius(350.0)
                .with_motion(MotionActivity::Automotive);

        while r_alpha.load(Ordering::Relaxed) {
            let sample = traj.next_sample();
            let req_id = pub_alpha.next_id().await;
            if let Ok(reply) = pub_alpha
                .send_with_reply(
                    req_id,
                    ClientBody::PublishLocation(PublishLocation {
                        session_id: sid_alpha.clone(),
                        samples: vec![sample],
                    }),
                    Duration::from_secs(3),
                )
                .await
                && let Some(ServerBody::PublishAck(_)) = reply.body
            {
                s_alpha.fetch_add(1, Ordering::Relaxed);
            }
            sleep(Duration::from_millis(500)).await;
        }
        let _ = pub_alpha.close().await;
    });

    // Task: Courier Beta (Sine wave path across North-South axis)
    let r_beta = Arc::clone(&running);
    let s_beta = Arc::clone(&beta_sent);
    let sid_beta = session_id.clone();
    let beta_task = tokio::spawn(async move {
        let mut traj =
            TrajectoryGenerator::new(TrajectoryKind::Sine, -12.0400, -77.0400, 15.0, 500)
                .with_motion(MotionActivity::Automotive);

        while r_beta.load(Ordering::Relaxed) {
            let sample = traj.next_sample();
            let req_id = pub_beta.next_id().await;
            if let Ok(reply) = pub_beta
                .send_with_reply(
                    req_id,
                    ClientBody::PublishLocation(PublishLocation {
                        session_id: sid_beta.clone(),
                        samples: vec![sample],
                    }),
                    Duration::from_secs(3),
                )
                .await
                && let Some(ServerBody::PublishAck(_)) = reply.body
            {
                s_beta.fetch_add(1, Ordering::Relaxed);
            }
            sleep(Duration::from_millis(500)).await;
        }
        let _ = pub_beta.close().await;
    });

    // Task: Dispatcher Reader (Receives fan-out stream)
    let r_disp = Arc::clone(&running);
    let d_rec = Arc::clone(&disp_received);
    let p_seen = Arc::clone(&publishers_seen);
    let alpha_id_str = mem_alpha_id.clone();
    let beta_id_str = mem_beta_id.clone();
    let is_quiet = config.quiet;

    let reader_task = tokio::spawn(async move {
        while r_disp.load(Ordering::Relaxed) {
            if let Ok(Some(msg)) = sub_client.next_message(Duration::from_millis(300)).await
                && let Some(ServerBody::LocationUpdate(update)) = msg.body
            {
                d_rec.fetch_add(1, Ordering::Relaxed);
                p_seen.lock().await.insert(update.member_id.clone());

                if !is_quiet {
                    let sample = update.sample.as_ref();
                    let pos = sample.and_then(|s| s.position.as_ref());
                    let seq = sample.map(|s| s.sequence).unwrap_or(0);
                    let spd = pos.and_then(|p| p.speed_mps).unwrap_or(0.0);

                    let courier_tag = if update.member_id == alpha_id_str {
                        "🔵 Courier Alpha (Van)"
                    } else if update.member_id == beta_id_str {
                        "🟢 Courier Beta  (Moto)"
                    } else {
                        "⚪ Unknown Courier"
                    };

                    println!(
                        "   [DISPATCHER] 📍 {courier_tag:<25} | Seq #{:<3} | Position: <redacted> | Spd: {:>4.1} m/s",
                        seq, spd
                    );
                }
            }
        }
        let _ = sub_client.close().await;
    });

    // Run for requested duration
    sleep(Duration::from_secs(config.duration_secs)).await;
    running.store(false, Ordering::Relaxed);

    let _ = tokio::join!(alpha_task, beta_task, reader_task);

    let final_alpha = alpha_sent.load(Ordering::Relaxed);
    let final_beta = beta_sent.load(Ordering::Relaxed);
    let final_disp = disp_received.load(Ordering::Relaxed);
    let final_seen = publishers_seen.lock().await.clone();

    if !config.quiet {
        println!();
        println!("📊 ===========================================================");
        println!("   Resumen y Estadísticas del Demo M1");
        println!("===========================================================");
        println!(
            "   📦 Puntos publicados por Courier Alpha: {:>4}",
            final_alpha
        );
        println!(
            "   🛵 Puntos publicados por Courier Beta:  {:>4}",
            final_beta
        );
        println!(
            "   📡 Puntos recibidos por el Dispatcher:  {:>4}",
            final_disp
        );
        println!(
            "   👥 Participantes detectados en stream:  {:>4}",
            final_seen.len()
        );

        let both_observed = final_seen.contains(&mem_alpha_id) && final_seen.contains(&mem_beta_id);
        if both_observed && final_alpha > 0 && final_beta > 0 && final_disp > 0 {
            println!("   🎉 RESULTADO: ÉXITO ROTUNDO! Stream bidireccional coalescido");
            println!("      verificado con 2 publicadores y 1 lector simultáneo.");
        } else {
            println!("   ⚠️ Advertencia: No se observaron muestras de ambos publicadores.");
        }
        println!("===========================================================");
    }

    Ok(DemoStats {
        alpha_sent: final_alpha,
        beta_sent: final_beta,
        dispatcher_received: final_disp,
        publishers_observed: final_seen,
    })
}
