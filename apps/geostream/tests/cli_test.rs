//! Integration tests for geostream CLI (FR-OPS-001).
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_geostream");

#[test]
fn test_version_subcommand() {
    // covers: FR-OPS-001
    let output = Command::new(BIN)
        .arg("version")
        .output()
        .expect("failed to execute geostream version");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("geostream"));
    assert!(stdout.contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn test_migrate_stub() {
    // covers: FR-OPS-001
    let output = Command::new(BIN)
        .arg("migrate")
        .output()
        .expect("failed to execute geostream migrate");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stdout} {stderr}");
    assert!(
        combined.to_lowercase().contains("not implemented")
            || combined.to_lowercase().contains("no implementado"),
        "expected not implemented message in: {combined}"
    );

    let dry_run_output = Command::new(BIN)
        .args(["migrate", "--dry-run"])
        .output()
        .expect("failed to execute geostream migrate --dry-run");
    let dry_run_str = format!(
        "{} {}",
        String::from_utf8_lossy(&dry_run_output.stdout),
        String::from_utf8_lossy(&dry_run_output.stderr)
    );
    assert!(
        dry_run_str.to_lowercase().contains("not implemented")
            || dry_run_str.to_lowercase().contains("no implementado"),
        "expected not implemented message in: {dry_run_str}"
    );
}

#[test]
fn test_admin_stub() {
    // covers: FR-OPS-001
    let output = Command::new(BIN)
        .args(["admin", "tenant", "list"])
        .output()
        .expect("failed to execute geostream admin");
    let combined = format!(
        "{} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.to_lowercase().contains("not implemented")
            || combined.to_lowercase().contains("no implementado"),
        "expected not implemented message in: {combined}"
    );
}

#[test]
fn test_serve_roles_validation() {
    // covers: FR-OPS-001
    let output = Command::new(BIN)
        .args(["serve", "--roles", "invalid_role"])
        .output()
        .expect("failed to execute geostream serve with invalid roles");
    assert!(!output.status.success());
}

#[tokio::test]
async fn test_serve_health_live() {
    // covers: FR-OPS-001
    // Choose an ephemeral port
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    let port = listener.local_addr().expect("local addr").port();
    drop(listener);

    let http_addr = format!("127.0.0.1:{port}");
    let mut child = Command::new(BIN)
        .args([
            "serve",
            "--http-addr",
            &http_addr,
            "--roles",
            "api,stream,worker",
        ])
        .spawn()
        .expect("failed to spawn geostream serve");

    // Poll until server responds or timeout
    let client = reqwest::Client::new();
    let url = format!("http://{http_addr}/health/live");
    let mut success = false;
    for _ in 0..50 {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        if let Ok(resp) = client.get(&url).send().await
            && resp.status().is_success()
        {
            let text = resp.text().await.unwrap_or_default();
            let body: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
            assert_eq!(body["status"], "ok");
            success = true;
            break;
        }
    }

    let _ = child.kill();
    let _ = child.wait();
    assert!(
        success,
        "server did not respond with 200 on /health/live within timeout"
    );
}
