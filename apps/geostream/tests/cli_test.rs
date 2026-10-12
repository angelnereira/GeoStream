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
fn test_migrate_command() {
    // covers: FR-OPS-001, FR-OPS-004
    // Without database URL, migrate fails with explicit error
    let output_no_db = Command::new(BIN)
        .arg("migrate")
        .env_remove("GS_DATABASE_URL")
        .output()
        .expect("failed to execute geostream migrate");
    assert!(!output_no_db.status.success());
    let stderr = String::from_utf8_lossy(&output_no_db.stderr);
    assert!(stderr.contains("database URL") || stderr.contains("GS_DATABASE_URL"));

    let db_url = std::env::var("GS_DATABASE_URL").unwrap_or_else(|_| {
        "postgres://geostream:geostream_dev_password@127.0.0.1:5432/geostream_test".to_string()
    });

    // Dry-run prints migration plan
    let dry_run_output = Command::new(BIN)
        .args(["migrate", "--dry-run", "--database-url", &db_url])
        .output()
        .expect("failed to execute geostream migrate --dry-run");
    assert!(dry_run_output.status.success());
    let dry_stdout = String::from_utf8_lossy(&dry_run_output.stdout);
    assert!(dry_stdout.contains("Migration plan (dry-run)"));

    // Real migrate succeeds
    let real_output = Command::new(BIN)
        .args(["migrate", "--database-url", &db_url])
        .output()
        .expect("failed to execute geostream migrate");
    assert!(real_output.status.success());
    let real_stdout = String::from_utf8_lossy(&real_output.stdout);
    assert!(real_stdout.contains("Successfully applied migrations"));
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

#[tokio::test]
async fn test_serve_auto_migrate_prod_rejected() {
    // covers: FR-OPS-004, docs/11 §4
    let db_url = std::env::var("GS_DATABASE_URL").unwrap_or_else(|_| {
        "postgres://geostream:geostream_dev_password@127.0.0.1:5432/geostream_test".to_string()
    });

    let output = Command::new(BIN)
        .args([
            "serve",
            "--database-url",
            &db_url,
            "--auto-migrate",
            "--env",
            "prod",
        ])
        .output()
        .expect("spawn geostream serve");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("GS_AUTO_MIGRATE=true is only allowed in GS_ENV=dev"),
        "expected auto_migrate rejection in prod, got: {stderr}"
    );
}

#[tokio::test]
async fn test_serve_with_database_schema_check() {
    // covers: FR-OPS-004, docs/11 §4
    let db_url = std::env::var("GS_DATABASE_URL").unwrap_or_else(|_| {
        "postgres://geostream:geostream_dev_password@127.0.0.1:5432/geostream_test".to_string()
    });

    // Ensure database is migrated first
    let mig_output = Command::new(BIN)
        .args(["migrate", "--database-url", &db_url])
        .output()
        .expect("run migrate");
    assert!(mig_output.status.success());

    // Ephemeral port
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    let port = listener.local_addr().expect("local addr").port();
    drop(listener);

    let http_addr = format!("127.0.0.1:{port}");
    let mut child = Command::new(BIN)
        .args([
            "serve",
            "--http-addr",
            &http_addr,
            "--database-url",
            &db_url,
            "--roles",
            "api",
        ])
        .spawn()
        .expect("spawn geostream serve with db check");

    let client = reqwest::Client::new();
    let url = format!("http://{http_addr}/health/live");
    let mut success = false;
    for _ in 0..50 {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        if let Ok(resp) = client.get(&url).send().await
            && resp.status().is_success()
        {
            success = true;
            break;
        }
    }

    let _ = child.kill();
    let _ = child.wait();
    assert!(
        success,
        "server with db schema check must start and respond on /health/live"
    );
}
