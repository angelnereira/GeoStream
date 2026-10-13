//! Integration tests for `geostream admin` against PostgreSQL (FR-ADM-001, FR-ADM-002, docs/16 §3).
#![forbid(unsafe_code)]

use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_geostream");

fn admin(args: &[&str]) -> (bool, String) {
    let output = Command::new(BIN)
        .arg("admin")
        .args(args)
        .output()
        .expect("failed to execute geostream admin");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    (output.status.success(), combined)
}

/// Runs a command that must succeed and returns its trimmed stdout.
fn run_ok(args: &[&str]) -> String {
    let (success, combined) = admin(args);
    assert!(success, "command failed: admin {args:?} -> {combined}");
    combined.trim().to_string()
}

fn test_database_url() -> String {
    std::env::var("GS_DATABASE_URL").unwrap_or_else(|_| {
        "postgres://geostream:geostream_dev_password@127.0.0.1:5432/geostream_test".to_string()
    })
}

#[test]
fn test_admin_help_documents_every_command_of_docs_16_3() {
    // covers: FR-ADM-001, docs/16 §3
    let output = Command::new(BIN)
        .args(["admin", "--help"])
        .output()
        .expect("geostream admin --help");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    for area in ["tenant", "app", "key", "policy", "signing-key", "session"] {
        assert!(
            stdout.contains(area),
            "missing admin area {area} in: {stdout}"
        );
    }
}

#[test]
fn test_admin_tenant_lifecycle_against_postgresql() {
    // covers: FR-ADM-001, docs/16 §3
    let db = test_database_url();
    let created = run_ok(&[
        "tenant",
        "create",
        "--name",
        "CLI Tenant",
        "--database-url",
        &db,
    ]);
    assert!(
        created.starts_with("ten_") && created.len() == 36,
        "tenant create must print the new id, got: {created}"
    );
    let tenant_id = created.trim().to_string();

    let listing = run_ok(&["tenant", "list", "--database-url", &db]);
    assert!(
        listing.contains(&tenant_id),
        "tenant list must include the created tenant"
    );

    run_ok(&[
        "tenant",
        "disable",
        "--tenant",
        &tenant_id,
        "--database-url",
        &db,
    ]);
    let listing = run_ok(&["tenant", "list", "--database-url", &db]);
    let line = listing
        .lines()
        .find(|line| line.contains(&tenant_id))
        .expect("tenant must still be listed");
    assert!(line.contains("DISABLED"), "expected DISABLED, got: {line}");

    run_ok(&[
        "tenant",
        "enable",
        "--tenant",
        &tenant_id,
        "--database-url",
        &db,
    ]);
    let listing = run_ok(&["tenant", "list", "--database-url", &db]);
    let line = listing
        .lines()
        .find(|line| line.contains(&tenant_id))
        .expect("tenant must still be listed");
    assert!(line.contains("ACTIVE"), "expected ACTIVE, got: {line}");
}

#[test]
fn test_admin_application_lifecycle_against_postgresql() {
    // covers: FR-ADM-001, docs/16 §3
    let db = test_database_url();
    let tenant = run_ok(&[
        "tenant",
        "create",
        "--name",
        "CLI App Tenant",
        "--database-url",
        &db,
    ]);

    let created = run_ok(&[
        "app",
        "create",
        "--tenant",
        &tenant,
        "--name",
        "cli-app",
        "--device-scopes",
        "stream:connect,sharing:write",
        "--database-url",
        &db,
    ]);
    assert!(
        created.starts_with("app_") && created.len() == 36,
        "app create must print the new id, got: {created}"
    );
    let app_id = created.trim().to_string();

    let listing = run_ok(&["app", "list", "--tenant", &tenant, "--database-url", &db]);
    assert!(
        listing.contains(&app_id),
        "app list must include the application"
    );
    assert!(listing.contains("stream:connect,sharing:write"));

    // An application with an empty name is rejected without touching the database.
    let (success, combined) = admin(&[
        "app",
        "create",
        "--tenant",
        &tenant,
        "--name",
        "",
        "--database-url",
        &db,
    ]);
    assert!(!success, "empty application name must fail: {combined}");
    assert!(combined.contains("application name must be between 1 and 100 characters"));

    // Unknown device scopes are rejected against docs/06 §6.
    let (success, combined) = admin(&[
        "app",
        "create",
        "--tenant",
        &tenant,
        "--name",
        "bad",
        "--device-scopes",
        "nope",
        "--database-url",
        &db,
    ]);
    assert!(!success, "unknown device scope must fail: {combined}");
    assert!(combined.contains("unknown application scope"));

    run_ok(&["app", "disable", "--app", &app_id, "--database-url", &db]);
    let listing = run_ok(&["app", "list", "--tenant", &tenant, "--database-url", &db]);
    let line = listing
        .lines()
        .find(|line| line.contains(&app_id))
        .expect("application must still be listed");
    assert!(line.contains("DISABLED"), "expected DISABLED, got: {line}");

    run_ok(&["app", "enable", "--app", &app_id, "--database-url", &db]);
    let listing = run_ok(&["app", "list", "--tenant", &tenant, "--database-url", &db]);
    let line = listing
        .lines()
        .find(|line| line.contains(&app_id))
        .expect("application must still be listed");
    assert!(line.contains("ACTIVE"), "expected ACTIVE, got: {line}");
}

#[test]
fn test_admin_key_create_prints_the_credential_exactly_once() {
    // covers: FR-ADM-001, FR-AUTH-001, docs/06 §3.1
    let db = test_database_url();
    let tenant = run_ok(&[
        "tenant",
        "create",
        "--name",
        "CLI Key Tenant",
        "--database-url",
        &db,
    ]);
    let app = run_ok(&[
        "app",
        "create",
        "--tenant",
        &tenant,
        "--name",
        "cli-key-app",
        "--database-url",
        &db,
    ]);

    let credential = run_ok(&[
        "key",
        "create",
        "--app",
        &app,
        "--label",
        "ci",
        "--expires-in",
        "90d",
        "--database-url",
        &db,
    ]);
    assert!(
        credential.starts_with("gsk_") && credential.contains('.'),
        "key create must print the credential, got: {credential}"
    );
    let secret = credential
        .rsplit('.')
        .next()
        .expect("credential carries a secret after the separator");

    // The listing never shows the secret.
    let listing = run_ok(&["key", "list", "--app", &app, "--database-url", &db]);
    assert!(listing.contains("ACTIVE"));
    assert!(listing.contains("ci"));
    assert!(
        !listing.contains(secret),
        "key list must never reveal the secret: {listing}"
    );

    // Invalid expiry values are rejected.
    let (success, combined) = admin(&[
        "key",
        "create",
        "--app",
        &app,
        "--expires-in",
        "90x",
        "--database-url",
        &db,
    ]);
    assert!(!success, "invalid expiry unit must fail: {combined}");
    assert!(combined.contains("invalid unit 'x'"));

    // Unknown scopes are rejected against docs/06 §3.4.
    let (success, combined) = admin(&[
        "key",
        "create",
        "--app",
        &app,
        "--scopes",
        "nope",
        "--database-url",
        &db,
    ]);
    assert!(!success, "unknown scope must fail: {combined}");
    assert!(combined.contains("unknown application scope"));

    // Revocation is terminal.
    let key_id = format!(
        "key_{}",
        credential
            .trim_start_matches("gsk_")
            .split('.')
            .next()
            .expect("key id")
    );
    run_ok(&["key", "revoke", "--key", &key_id, "--database-url", &db]);
    let listing = run_ok(&["key", "list", "--app", &app, "--database-url", &db]);
    let line = listing
        .lines()
        .find(|line| line.contains(&key_id))
        .expect("key must still be listed");
    assert!(line.contains("REVOKED"), "expected REVOKED, got: {line}");
    assert!(
        !line.contains("gsk_"),
        "the listing must never show credentials"
    );

    let (success, combined) = admin(&["key", "revoke", "--key", &key_id, "--database-url", &db]);
    assert!(success, "revoking twice must stay consistent: {combined}");
}

#[test]
fn test_admin_policy_get_and_set_validate_ranges() {
    // covers: FR-ADM-002, docs/17 §4
    let db = test_database_url();
    let tenant = run_ok(&[
        "tenant",
        "create",
        "--name",
        "CLI Policy Tenant",
        "--database-url",
        &db,
    ]);

    let effective = run_ok(&["policy", "get", "--tenant", &tenant, "--database-url", &db]);
    assert!(
        effective.contains("\"max_devices_per_subject\": 10"),
        "absent keys must show the default, got: {effective}"
    );

    let updated = run_ok(&[
        "policy",
        "set",
        "--tenant",
        &tenant,
        "max_devices_per_subject=4",
        "--database-url",
        &db,
    ]);
    assert!(updated.contains("\"max_devices_per_subject\": 4"));
    assert!(updated.contains("\"publish_burst\": 5"));

    let (success, combined) = admin(&[
        "policy",
        "set",
        "--tenant",
        &tenant,
        "max_devices_per_subject=0",
        "--database-url",
        &db,
    ]);
    assert!(!success, "out of range value must fail: {combined}");
    assert!(
        combined.contains("invalid tenant policy"),
        "expected explicit message, got: {combined}"
    );

    let (success, combined) = admin(&[
        "policy",
        "set",
        "--tenant",
        &tenant,
        "unknown_key=1",
        "--database-url",
        &db,
    ]);
    assert!(!success, "unknown policy key must fail: {combined}");
    assert!(combined.contains("unknown policy key"));

    let (success, combined) = admin(&[
        "policy",
        "set",
        "--tenant",
        &tenant,
        "history_allowed",
        "--database-url",
        &db,
    ]);
    assert!(!success, "assignment without value must fail: {combined}");

    let effective = run_ok(&["policy", "get", "--tenant", &tenant, "--database-url", &db]);
    assert!(
        effective.contains("\"max_devices_per_subject\": 4"),
        "rejected assignments must not modify the policy: {effective}"
    );
}

#[test]
fn test_admin_signing_key_generate_creates_the_pair_once() {
    // covers: FR-ADM-001, FR-AUTH-004, docs/06 §4.4
    let dir = std::env::temp_dir().join(format!(
        "geostream-admin-signing-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time")
            .as_nanos()
    ));
    let out = dir.to_str().expect("path");

    let output = run_ok(&[
        "signing-key",
        "generate",
        "--kid",
        "cli-test-key",
        "--out",
        out,
    ]);
    assert!(output.contains("cli-test-key.key.pem"));
    assert!(output.contains("cli-test-key.pub.pem"));
    assert!(dir.join("cli-test-key.key.pem").exists());
    assert!(dir.join("cli-test-key.pub.pem").exists());
    assert!(
        std::fs::read_to_string(dir.join("cli-test-key.pub.pem"))
            .expect("read public key")
            .starts_with("-----BEGIN PUBLIC KEY-----")
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(dir.join("cli-test-key.key.pem"))
            .expect("private key metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600, "private key must be written with mode 0600");
    }

    let (success, combined) = admin(&[
        "signing-key",
        "generate",
        "--kid",
        "cli-test-key",
        "--out",
        out,
    ]);
    assert!(
        !success,
        "existing keys must never be overwritten: {combined}"
    );
    assert!(combined.contains("already exists"));

    let (success, combined) = admin(&["signing-key", "generate", "--kid", "Bad_Kid", "--out", out]);
    assert!(!success, "invalid kid must fail: {combined}");
    assert!(combined.contains("invalid key id"));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_admin_session_close_reports_the_f3_deferral() {
    // covers: FR-ADM-001, docs/16 §3
    let (success, combined) = admin(&[
        "session",
        "close",
        "--session",
        "ses_0199c0ffee0000000000000000000000",
    ]);
    assert!(
        !success,
        "session close must report that it is not implemented yet"
    );
    assert!(
        combined.contains("not implemented yet") && combined.contains("T-F3-03"),
        "unexpected message: {combined}"
    );
}

#[test]
fn test_admin_requires_a_database_url() {
    // covers: FR-ADM-001
    let output = Command::new(BIN)
        .args(["admin", "tenant", "list"])
        .env_remove("GS_DATABASE_URL")
        .output()
        .expect("geostream admin tenant list without database url");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("database URL") || stderr.contains("GS_DATABASE_URL"),
        "expected explicit message, got: {stderr}"
    );
}
