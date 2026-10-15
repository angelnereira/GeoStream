//! REST API helpers for gs-devclient.
#![forbid(unsafe_code)]

use anyhow::{Context, Result, bail};
use reqwest::Client;
use serde::{Deserialize, Serialize};

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct StreamTicketResponse {
    pub ticket: String,
    pub stream_url: String,
    pub expires_at: String,
}

#[derive(Debug, Clone, Serialize)]
struct SetSharingRequest {
    sharing: bool,
}

/// Requests a single-use stream ticket using a bearer token.
pub async fn request_stream_ticket(base_url: &str, token: &str) -> Result<StreamTicketResponse> {
    let client = Client::builder().build()?;
    let trimmed = base_url.trim_end_matches('/');
    let url = format!("{trimmed}/v1/stream/tickets");

    let resp = client
        .post(&url)
        .bearer_auth(token)
        .send()
        .await
        .with_context(|| format!("Failed to send request to {url}"))?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        bail!("Failed to obtain stream ticket (status {status}): {body}");
    }

    let ticket_resp = resp
        .json::<StreamTicketResponse>()
        .await
        .context("Failed to deserialize StreamTicketResponse")?;

    Ok(ticket_resp)
}

/// Enables or disables location sharing for the current member.
pub async fn set_sharing(
    base_url: &str,
    session_id: &str,
    token: &str,
    sharing: bool,
) -> Result<()> {
    let client = Client::builder().build()?;
    let trimmed = base_url.trim_end_matches('/');
    let url = format!("{trimmed}/v1/sessions/{session_id}/members/me/sharing");

    let resp = client
        .put(&url)
        .bearer_auth(token)
        .json(&SetSharingRequest { sharing })
        .send()
        .await
        .with_context(|| format!("Failed to set sharing at {url}"))?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        bail!("Failed to set sharing (status {status}): {body}");
    }

    Ok(())
}

/// Fetches the latest retained location snapshot for a session.
pub async fn get_snapshot(
    base_url: &str,
    session_id: &str,
    token: &str,
) -> Result<serde_json::Value> {
    let client = Client::builder().build()?;
    let trimmed = base_url.trim_end_matches('/');
    let url = format!("{trimmed}/v1/sessions/{session_id}/snapshot");

    let resp = client
        .get(&url)
        .bearer_auth(token)
        .send()
        .await
        .with_context(|| format!("Failed to get snapshot from {url}"))?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        bail!("Failed to get snapshot (status {status}): {body}");
    }

    let json = resp
        .json::<serde_json::Value>()
        .await
        .context("Failed to deserialize snapshot JSON")?;

    Ok(json)
}
