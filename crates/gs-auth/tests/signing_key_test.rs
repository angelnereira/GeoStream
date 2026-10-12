//! Unit tests for Ed25519 signing key generation (docs/06 §4.4).
#![forbid(unsafe_code)]

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use gs_auth::signing_key::{
    PRIVATE_KEY_SUFFIX, PUBLIC_KEY_SUFFIX, SigningKeyError, generate, parse_private_pem,
    parse_public_pem, validate_kid,
};

static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_dir() -> PathBuf {
    let unique = format!(
        "gs-auth-signing-key-{}-{}",
        std::process::id(),
        TEST_COUNTER.fetch_add(1, Ordering::SeqCst)
    );
    let dir = std::env::temp_dir().join(unique);
    fs::create_dir_all(&dir).expect("create test dir");
    dir
}

#[test]
fn test_generate_produces_parseable_pkcs8_and_spki_pem() {
    // covers: FR-AUTH-004, docs/06 §4.4
    let generated = generate("dev-key-1").expect("generate key pair");

    assert!(
        generated
            .public_pem
            .starts_with("-----BEGIN PUBLIC KEY-----\n")
    );
    assert!(generated.public_pem.ends_with("-----END PUBLIC KEY-----\n"));
    assert!(
        generated.public_pem.lines().count() >= 3,
        "public PEM must have a wrapped base64 body"
    );

    let signing_key = parse_private_pem(generated.private_pem()).expect("parse private PEM");
    let verifying_key = parse_public_pem(&generated.public_pem).expect("parse public PEM");
    assert_eq!(signing_key.verifying_key(), verifying_key);
}

#[test]
fn test_debug_redacts_the_private_pem() {
    // covers: docs/10 §8.2, docs/06 §4.4
    let generated = generate("dev-key-2").expect("generate key pair");
    let rendered = format!("{generated:?}");
    assert!(rendered.contains("\"<redacted>\""));
    assert!(!rendered.contains("BEGIN PRIVATE KEY"));
}

#[test]
fn test_write_pair_creates_both_files_with_private_only_owner_accessible() {
    // covers: FR-AUTH-004, docs/06 §4.4
    let dir = temp_dir();
    let generated = generate("dev-key-3").expect("generate key pair");
    let (private_path, public_path) = generated.write_pair(&dir).expect("write pair");

    assert!(private_path.ends_with(format!("dev-key-3.{PRIVATE_KEY_SUFFIX}")));
    assert!(public_path.ends_with(format!("dev-key-3.{PUBLIC_KEY_SUFFIX}")));
    assert_eq!(
        fs::read_to_string(&private_path).expect("read private file"),
        generated.private_pem()
    );
    assert_eq!(
        fs::read_to_string(&public_path).expect("read public file"),
        generated.public_pem
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let private_mode = fs::metadata(&private_path)
            .expect("private file metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(private_mode, 0o600, "private key file must be 0600");
    }

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn test_write_pair_rejects_existing_files() {
    // covers: FR-AUTH-004, docs/06 §4.4
    let dir = temp_dir();
    let generated = generate("dev-key-4").expect("generate key pair");
    generated
        .write_pair(&dir)
        .expect("first write must succeed");

    let second = generate("dev-key-4").expect("generate a second pair with the same kid");
    assert!(matches!(
        second.write_pair(&dir),
        Err(SigningKeyError::AlreadyExists { .. })
    ));

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn test_validate_kid_enforces_the_documented_pattern() {
    // covers: FR-AUTH-004, docs/06 §4.4
    assert!(validate_kid("dev-key-1").is_ok());
    assert!(validate_kid("a").is_ok());

    assert!(validate_kid("").is_err());
    assert!(validate_kid("UPPER").is_err());
    assert!(validate_kid("under_score").is_err());
    assert!(validate_kid(&"k".repeat(65)).is_err());
}
