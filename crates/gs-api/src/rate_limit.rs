//! Rate limiting for REST endpoints and invitations (docs/07 §8, docs/11 §3, docs/16 §4.2, FR-API-004).
#![forbid(unsafe_code)]

use std::time::Duration;

use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use gs_service::EphemeralStore;
use gs_service::gs_domain::error::ErrorCode;
use time::OffsetDateTime;

use crate::problem::{Problem, ProblemResponse};

/// Canonical scope names for rate limiting (docs/16 §4.2).
pub const SCOPE_REST_DEVICE: &str = "rest_device";
pub const SCOPE_REST_APP: &str = "rest_app";
pub const SCOPE_INVITE_REDEEM: &str = "invite_redeem";
pub const SCOPE_AUTH_IP: &str = "auth_ip";

/// Metric name for rate limiting counter (docs/16 §4.2).
pub const METRIC_RATE_LIMITED_TOTAL: &str = "gs_rate_limited_total";

/// Rate limit parameters configuration (docs/17 §2.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateLimitConfig {
    /// Limit for device requests per minute (GS_REST_RATE_LIMIT_DEVICE_RPM).
    pub device_rpm: u32,
    /// Limit for application requests per minute (GS_REST_RATE_LIMIT_APP_RPM).
    pub app_rpm: u32,
    /// Limit for invitation redemption attempts per minute (GS_INVITE_REDEEM_RPM).
    pub invite_redeem_rpm: u32,
    /// Limit for authentication failures per minute per IP (GS_AUTH_FAILURE_RPM_PER_IP).
    pub auth_failure_rpm: u32,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            device_rpm: 120,
            app_rpm: 6000,
            invite_redeem_rpm: 10,
            auth_failure_rpm: 60,
        }
    }
}

/// Metadata about the current rate limit window (docs/07 §8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateLimitInfo {
    /// Configured limit for the scope.
    pub limit: u32,
    /// Remaining allowed requests in the current window.
    pub remaining: u64,
    /// Seconds until the current window resets.
    pub reset_seconds: u64,
}

/// Returned when a rate limit is exceeded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateLimitExceeded {
    pub limit: u32,
    pub retry_after: u64,
}

/// Checks and increments a rate limit counter using a fixed 60s window (docs/07 §8, docs/11 §3).
pub async fn check_rate_limit<E: EphemeralStore>(
    ephemeral: &E,
    key_prefix: &str,
    scope: &str,
    identifier: &str,
    limit: u32,
    now: OffsetDateTime,
) -> Result<RateLimitInfo, RateLimitExceeded> {
    let now_unix = now.unix_timestamp();
    let window_bucket = now_unix / 60;
    let rl_key = format!("{key_prefix}:rl:{scope}:{identifier}:{window_bucket}");

    // Counter TTL 120s per docs/11 §3
    let count = ephemeral
        .increment_counter(&rl_key, Duration::from_secs(120))
        .await
        .map_err(|_| RateLimitExceeded {
            limit,
            retry_after: 60,
        })?;

    let seconds_into_window = (now_unix % 60) as u64;
    let reset_seconds = (60 - seconds_into_window).max(1);

    if count > limit as u64 {
        metrics::counter!(METRIC_RATE_LIMITED_TOTAL, "scope" => scope.to_string()).increment(1);
        Err(RateLimitExceeded {
            limit,
            retry_after: reset_seconds,
        })
    } else {
        let remaining = (limit as u64).saturating_sub(count);
        Ok(RateLimitInfo {
            limit,
            remaining,
            reset_seconds,
        })
    }
}

/// Builds an HTTP 429 Problem Details response for rate limit violations (FR-API-004, docs/07 §8).
pub fn rate_limit_problem_response(
    retry_after: u64,
    limit: u32,
    scope: &str,
    request_id: &str,
) -> Response {
    let detail = format!("rate limit exceeded for {scope}; retry after {retry_after}s");
    let problem = Problem::new(ErrorCode::RateLimited, request_id).with_detail(detail);
    let mut resp = (StatusCode::TOO_MANY_REQUESTS, ProblemResponse::new(problem)).into_response();

    // Mandatory Retry-After header (docs/07 §8, OpenAPI components/headers/RetryAfter)
    if let Ok(v) = HeaderValue::from_str(&retry_after.to_string()) {
        resp.headers_mut().insert(header::RETRY_AFTER, v);
    }

    // Informative RateLimit-* headers (docs/07 §8)
    if let Ok(v) = HeaderValue::from_str(&limit.to_string()) {
        resp.headers_mut()
            .insert(HeaderName::from_static("ratelimit-limit"), v);
    }
    resp.headers_mut().insert(
        HeaderName::from_static("ratelimit-remaining"),
        HeaderValue::from_static("0"),
    );
    if let Ok(v) = HeaderValue::from_str(&retry_after.to_string()) {
        resp.headers_mut()
            .insert(HeaderName::from_static("ratelimit-reset"), v);
    }
    if let Ok(v) = HeaderValue::from_str(request_id) {
        resp.headers_mut()
            .insert(HeaderName::from_static("x-request-id"), v);
    }
    resp
}

/// Applies informative RateLimit-* headers to a response (docs/07 §8).
pub fn apply_rate_limit_headers(response: &mut Response, info: &RateLimitInfo) {
    if let Ok(v) = HeaderValue::from_str(&info.limit.to_string()) {
        response
            .headers_mut()
            .insert(HeaderName::from_static("ratelimit-limit"), v);
    }
    if let Ok(v) = HeaderValue::from_str(&info.remaining.to_string()) {
        response
            .headers_mut()
            .insert(HeaderName::from_static("ratelimit-remaining"), v);
    }
    if let Ok(v) = HeaderValue::from_str(&info.reset_seconds.to_string()) {
        response
            .headers_mut()
            .insert(HeaderName::from_static("ratelimit-reset"), v);
    }
}

/// Extracts client IP address from headers (X-Forwarded-For, X-Real-IP, or default).
pub fn extract_client_ip(headers: &HeaderMap) -> String {
    if let Some(forwarded) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok())
        && let Some(first) = forwarded.split(',').next()
    {
        let trimmed = first.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    if let Some(real_ip) = headers.get("x-real-ip").and_then(|v| v.to_str().ok()) {
        let trimmed = real_ip.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    "127.0.0.1".to_string()
}

/// Records an authentication failure for a client IP and returns a 429 response if exceeded.
pub async fn record_auth_failure_and_check<E: EphemeralStore>(
    ephemeral: &E,
    key_prefix: &str,
    client_ip: &str,
    limit: u32,
    request_id: &str,
) -> Option<Response> {
    let now = OffsetDateTime::now_utc();
    match check_rate_limit(ephemeral, key_prefix, SCOPE_AUTH_IP, client_ip, limit, now).await {
        Ok(_) => None,
        Err(exceeded) => Some(rate_limit_problem_response(
            exceeded.retry_after,
            exceeded.limit,
            SCOPE_AUTH_IP,
            request_id,
        )),
    }
}
