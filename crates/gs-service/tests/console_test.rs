//! Console sign-in flow against PostgreSQL with an in-memory mailer, a stub of the e-mail
//! provider and a log capture (T-F15-17, FR-CON-001, FR-CON-009 to FR-CON-011, ADR-0036).
#![forbid(unsafe_code)]

use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gs_auth::{ConsoleKeys, EmailAddress, SecretString};
use gs_service::console::{ConsoleAuth, ConsoleError, ConsoleSettings, EmailBackend, MailError};
use gs_service::migrate::{connect_pool, run_migrations};
use uuid::Uuid;

/// The daily e-mail budget is global to the database. The test that spends it takes the write
/// side; every other test takes the read side so none of them runs while the budget is measured.
static BUDGET: tokio::sync::RwLock<()> = tokio::sync::RwLock::const_new(());

fn test_database_url() -> String {
    std::env::var("GS_DATABASE_URL").unwrap_or_else(|_| {
        "postgres://geostream:geostream_dev_password@127.0.0.1:5432/geostream_test".to_string()
    })
}

fn settings() -> ConsoleSettings {
    ConsoleSettings {
        otp_length: 6,
        otp_ttl: Duration::from_secs(600),
        otp_max_attempts: 3,
        otp_cooldown: Duration::from_secs(60),
        session_ttl: Duration::from_secs(3600),
        email_daily_cap: 10_000,
        max_orgs_per_account: 3,
    }
}

async fn service_with(settings: ConsoleSettings) -> (ConsoleAuth, Arc<EmailBackend>) {
    let pool = connect_pool(&test_database_url()).await.expect("connect");
    run_migrations(&pool).await.expect("migrate");
    let mailer = Arc::new(EmailBackend::memory());
    let keys = ConsoleKeys::new(&[42u8; 32]).expect("key");
    (
        ConsoleAuth::new(pool, keys, settings, Arc::clone(&mailer)),
        mailer,
    )
}

async fn service() -> (ConsoleAuth, Arc<EmailBackend>) {
    service_with(settings()).await
}

fn address() -> String {
    format!("Person.{}@Example.Test", Uuid::new_v4().simple())
}

/// The code of the last message sent to `to`.
fn last_code(mailer: &EmailBackend, to: &str) -> String {
    let normalized = to.to_ascii_lowercase();
    let mail = mailer
        .sent()
        .into_iter()
        .rev()
        .find(|m| m.to == normalized)
        .expect("a mail was sent");
    let marker = "es: ";
    let start = mail.body.find(marker).expect("code marker") + marker.len();
    mail.body[start..start + 6].to_string()
}

#[tokio::test]
async fn test_fr_con_001_requesting_a_code_sends_one_mail_and_signs_a_new_account_up() {
    let _budget = BUDGET.read().await;
    // covers: FR-CON-001
    let (auth, mailer) = service().await;
    let to = address();
    auth.request_code(&to).await.expect("request");
    let sent = mailer.sent();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].to, to.to_ascii_lowercase());
    assert!(
        !sent[0].body.contains("http"),
        "no links in the message: nothing to phish with"
    );

    let code = last_code(&mailer, &to);
    let opened = auth.verify_code(&to, &code, true).await.expect("verify");
    assert!(opened.created, "first redemption creates the account");
    assert!(opened.token.starts_with("gcs_"));
    assert_eq!(opened.account.email, to.to_ascii_lowercase());

    // The token authenticates, and is not the stored value.
    let account = auth
        .authenticate(&opened.token)
        .await
        .expect("authenticate");
    assert_eq!(account.id, opened.account.id);

    // Signing in again is the same ceremony and finds the same account.
    auth.request_code(&to).await.ok();
}

#[tokio::test]
async fn test_fr_con_001_a_known_account_signs_in_without_creating_another() {
    let _budget = BUDGET.read().await;
    // covers: FR-CON-001
    let mut fast = settings();
    fast.otp_cooldown = Duration::from_secs(0);
    let (auth, mailer) = service_with(fast).await;
    let to = address();
    auth.request_code(&to).await.unwrap();
    let first = auth
        .verify_code(&to, &last_code(&mailer, &to), true)
        .await
        .unwrap();
    auth.request_code(&to).await.unwrap();
    let second = auth
        .verify_code(&to, &last_code(&mailer, &to), false)
        .await
        .expect("an existing account does not need the terms again");
    assert!(!second.created);
    assert_eq!(first.account.id, second.account.id);
    assert_ne!(
        *first.token, *second.token,
        "every sign-in is a new session"
    );
}

#[tokio::test]
async fn test_fr_con_001_the_terms_are_required_for_a_new_account_but_the_code_survives() {
    let _budget = BUDGET.read().await;
    // covers: FR-CON-001
    let (auth, mailer) = service().await;
    let to = address();
    auth.request_code(&to).await.unwrap();
    let code = last_code(&mailer, &to);
    assert_eq!(
        auth.verify_code(&to, &code, false).await.unwrap_err(),
        ConsoleError::TermsRequired
    );
    // Same code, now with the terms accepted.
    assert!(auth.verify_code(&to, &code, true).await.is_ok());
}

#[tokio::test]
async fn test_fr_con_001_every_failure_of_the_code_is_the_same_unauthenticated() {
    let _budget = BUDGET.read().await;
    // covers: FR-CON-001, 10 T-41, T-42
    let (auth, mailer) = service().await;
    let to = address();
    auth.request_code(&to).await.unwrap();
    let code = last_code(&mailer, &to);
    let wrong = if code == "000000" { "000001" } else { "000000" };

    // Wrong code for an address with a live code, and any code for an address with none.
    let wrong_code = auth.verify_code(&to, wrong, true).await.unwrap_err();
    let nobody = auth
        .verify_code(&address(), "123456", true)
        .await
        .unwrap_err();
    assert_eq!(wrong_code, ConsoleError::Unauthenticated);
    assert_eq!(nobody, ConsoleError::Unauthenticated);
    assert_eq!(wrong_code, nobody, "indistinguishable");
}

#[tokio::test]
async fn test_fr_con_001_the_code_dies_after_the_attempt_limit_even_if_then_correct() {
    let _budget = BUDGET.read().await;
    // covers: FR-CON-001, 10 T-41
    let (auth, mailer) = service().await;
    let to = address();
    auth.request_code(&to).await.unwrap();
    let code = last_code(&mailer, &to);
    let wrong = if code == "000000" { "000001" } else { "000000" };
    for _ in 0..3 {
        assert_eq!(
            auth.verify_code(&to, wrong, true).await.unwrap_err(),
            ConsoleError::Unauthenticated
        );
    }
    assert_eq!(
        auth.verify_code(&to, &code, true).await.unwrap_err(),
        ConsoleError::Unauthenticated,
        "the right code no longer works after three failures"
    );
}

#[tokio::test]
async fn test_fr_con_001_a_code_cannot_be_used_twice() {
    let _budget = BUDGET.read().await;
    // covers: FR-CON-001
    let (auth, mailer) = service().await;
    let to = address();
    auth.request_code(&to).await.unwrap();
    let code = last_code(&mailer, &to);
    assert!(auth.verify_code(&to, &code, true).await.is_ok());
    assert_eq!(
        auth.verify_code(&to, &code, true).await.unwrap_err(),
        ConsoleError::Unauthenticated
    );
}

#[tokio::test]
async fn test_fr_con_001_a_code_only_works_for_its_own_address() {
    let _budget = BUDGET.read().await;
    // covers: FR-CON-001, 10 T-41
    let (auth, mailer) = service().await;
    let (alice, mallory) = (address(), address());
    auth.request_code(&alice).await.unwrap();
    auth.request_code(&mallory).await.unwrap();
    let alices_code = last_code(&mailer, &alice);
    assert_eq!(
        auth.verify_code(&mallory, &alices_code, true)
            .await
            .unwrap_err(),
        ConsoleError::Unauthenticated,
        "Alice's code must not open Mallory's sign-in"
    );
}

#[tokio::test]
async fn test_fr_con_009_asking_twice_inside_the_cooldown_sends_one_mail() {
    let _budget = BUDGET.read().await;
    // covers: FR-CON-009
    let (auth, mailer) = service().await;
    let to = address();
    auth.request_code(&to).await.unwrap();
    auth.request_code(&to).await.expect("same answer");
    auth.request_code(&to).await.expect("same answer");
    assert_eq!(mailer.sent().len(), 1);
}

#[tokio::test]
async fn test_fr_con_009_the_answer_is_the_same_for_a_known_and_an_unknown_address() {
    let _budget = BUDGET.read().await;
    // covers: FR-CON-009, 10 T-42
    let mut fast = settings();
    fast.otp_cooldown = Duration::from_secs(0);
    let (auth, mailer) = service_with(fast).await;
    let known = address();
    auth.request_code(&known).await.unwrap();
    auth.verify_code(&known, &last_code(&mailer, &known), true)
        .await
        .unwrap();

    let known_answer = auth.request_code(&known).await;
    let unknown_answer = auth.request_code(&address()).await;
    assert_eq!(known_answer, unknown_answer);
    assert_eq!(known_answer, Ok(()));
}

#[tokio::test]
async fn test_fr_con_009_malformed_addresses_are_rejected_before_any_work() {
    let _budget = BUDGET.read().await;
    // covers: FR-CON-009
    let (auth, mailer) = service().await;
    for bad in [
        "",
        "no-at-sign",
        "a@b",
        "x@example.test\r\nBcc: v@example.test",
    ] {
        assert_eq!(
            auth.request_code(bad).await.unwrap_err(),
            ConsoleError::InvalidEmail,
            "{bad:?}"
        );
    }
    assert!(mailer.sent().is_empty());
    assert_eq!(
        auth.verify_code(&address(), "abc", true).await.unwrap_err(),
        ConsoleError::InvalidCodeFormat
    );
}

#[tokio::test]
async fn test_fr_con_009_the_daily_budget_stops_the_mail_without_changing_the_answer() {
    let _budget = BUDGET.write().await;
    // covers: FR-CON-009, 10 T-43
    let (auth, mailer) = service_with(ConsoleSettings {
        email_daily_cap: 2,
        ..settings()
    })
    .await;
    sqlx::query("DELETE FROM console_email_counters;")
        .execute(&connect_pool(&test_database_url()).await.unwrap())
        .await
        .unwrap();
    for _ in 0..5 {
        assert_eq!(auth.request_code(&address()).await, Ok(()));
    }
    assert_eq!(mailer.sent().len(), 2, "only the budget was sent");
}

#[tokio::test]
async fn test_fr_con_010_logout_revokes_the_session() {
    let _budget = BUDGET.read().await;
    // covers: FR-CON-010
    let (auth, mailer) = service().await;
    let to = address();
    auth.request_code(&to).await.unwrap();
    let opened = auth
        .verify_code(&to, &last_code(&mailer, &to), true)
        .await
        .unwrap();
    auth.logout(&opened.token).await.unwrap();
    assert_eq!(
        auth.authenticate(&opened.token).await.unwrap_err(),
        ConsoleError::Unauthenticated
    );
    for bad in [
        "",
        "Bearer x",
        "gsk_0123456789abcdef0123456789abcdef",
        "gcs_short",
    ] {
        assert_eq!(
            auth.authenticate(bad).await.unwrap_err(),
            ConsoleError::Unauthenticated
        );
    }
}

#[tokio::test]
async fn test_fr_con_010_the_database_holds_neither_the_code_nor_the_token() {
    let _budget = BUDGET.read().await;
    // covers: FR-CON-010, docs/10 §5.6
    let (auth, mailer) = service().await;
    let to = address();
    auth.request_code(&to).await.unwrap();
    let code = last_code(&mailer, &to);
    let opened = auth.verify_code(&to, &code, true).await.unwrap();

    let pool = connect_pool(&test_database_url()).await.unwrap();
    let stored_codes: Vec<Vec<u8>> = sqlx::query_scalar("SELECT code_hmac FROM console_otp_codes;")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert!(
        stored_codes
            .iter()
            .all(|c| c.len() == 32 && c != code.as_bytes())
    );
    let stored_tokens: Vec<Vec<u8>> =
        sqlx::query_scalar("SELECT token_sha256 FROM console_sessions;")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert!(
        stored_tokens
            .iter()
            .all(|t| t.len() == 32 && t.as_slice() != opened.token.as_bytes())
    );
}

#[tokio::test]
async fn test_deleting_an_account_ends_its_sessions() {
    let _budget = BUDGET.read().await;
    // covers: FR-CON-010, docs/10 §7
    let (auth, mailer) = service().await;
    let to = address();
    auth.request_code(&to).await.unwrap();
    let opened = auth
        .verify_code(&to, &last_code(&mailer, &to), true)
        .await
        .unwrap();
    auth.delete_account(opened.account.id).await.unwrap();
    assert_eq!(
        auth.authenticate(&opened.token).await.unwrap_err(),
        ConsoleError::Unauthenticated
    );
}

// ---------- logs: neither the address nor the code may be written (FR-CON-011) ----------

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl Write for Capture {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("capture lock").extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Capture {
    type Writer = Capture;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

#[tokio::test(flavor = "current_thread")]
async fn test_fr_con_011_neither_the_email_nor_the_code_reaches_a_log() {
    let _budget = BUDGET.read().await;
    // covers: FR-CON-011, docs/10 §8.2
    let capture = Capture::default();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_writer(capture.clone())
        .with_ansi(false)
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);

    let (auth, mailer) = service().await;
    let to = address();
    auth.request_code(&to).await.unwrap();
    let code = last_code(&mailer, &to);
    let wrong = if code == "000000" { "000001" } else { "000000" };
    let _ = auth.verify_code(&to, wrong, true).await;
    let opened = auth.verify_code(&to, &code, true).await.unwrap();
    let _ = auth.authenticate(&opened.token).await;
    // A delivery failure logs too, and that is the path most likely to leak an address: drive it
    // through the service itself with a provider that cannot be reached.
    let broken = EmailBackend::resend(
        "http://127.0.0.1:1",
        SecretString::new("re_key_that_must_not_leak"),
        "GeoStream <no-reply@example.test>",
    )
    .unwrap();
    let pool = connect_pool(&test_database_url()).await.unwrap();
    let failing = ConsoleAuth::new(
        pool,
        ConsoleKeys::new(&[42u8; 32]).unwrap(),
        settings(),
        Arc::new(broken),
    );
    let other = address();
    assert_eq!(
        failing.request_code(&other).await,
        Ok(()),
        "a delivery failure must not change the answer"
    );
    // The provider adapter on its own must not log either.
    let direct = EmailBackend::resend(
        "http://127.0.0.1:1",
        SecretString::new("re_key_that_must_not_leak"),
        "GeoStream <no-reply@example.test>",
    )
    .unwrap();
    let email = EmailAddress::parse(&to).unwrap();
    assert_eq!(
        direct
            .send_code(&email, &code, Duration::from_secs(600))
            .await,
        Err(MailError::Unreachable)
    );

    let logs = String::from_utf8(capture.0.lock().unwrap().clone()).expect("utf-8 logs");
    let local = to.split('@').next().unwrap().to_ascii_lowercase();
    for secret in [
        local.as_str(),
        "example.test",
        &code,
        wrong,
        &*opened.token,
        "re_key_that_must_not_leak",
    ] {
        assert!(
            !logs
                .to_ascii_lowercase()
                .contains(&secret.to_ascii_lowercase()),
            "a secret reached the logs: {secret:?}\n{logs}"
        );
    }
}

// ---------- the e-mail provider adapter ----------

#[derive(Clone, Default)]
struct Seen(Arc<Mutex<Vec<(String, String, String)>>>);

async fn stub_provider(status: u16, seen: Seen) -> String {
    use axum::Router;
    use axum::extract::State;
    use axum::http::{HeaderMap, StatusCode};
    use axum::routing::post;

    async fn handler(
        State((status, seen)): State<(u16, Seen)>,
        headers: HeaderMap,
        body: String,
    ) -> (StatusCode, String) {
        let auth = headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string();
        let ctype = headers
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string();
        seen.0.lock().unwrap().push((auth, ctype, body));
        // A real provider echoes the recipient in its error body: the adapter must drop it.
        (
            StatusCode::from_u16(status).unwrap(),
            "{\"message\":\"cannot deliver to someone@example.test\"}".to_string(),
        )
    }
    let app = Router::new()
        .route("/emails", post(handler))
        .with_state((status, seen));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    url
}

#[tokio::test]
async fn test_fr_con_001_the_provider_receives_a_bearer_json_message_with_the_code() {
    let _budget = BUDGET.read().await;
    // covers: FR-CON-001
    let seen = Seen::default();
    let url = stub_provider(200, seen.clone()).await;
    let mailer = EmailBackend::resend(
        &url,
        SecretString::new("re_test_key"),
        "GeoStream <no-reply@example.test>",
    )
    .unwrap();
    let to = EmailAddress::parse("ana@example.test").unwrap();
    mailer
        .send_code(&to, "482913", Duration::from_secs(600))
        .await
        .expect("delivered");
    let (auth, ctype, body) = seen.0.lock().unwrap()[0].clone();
    assert_eq!(auth, "Bearer re_test_key");
    assert_eq!(ctype, "application/json");
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(json["from"], "GeoStream <no-reply@example.test>");
    assert_eq!(json["to"], serde_json::json!(["ana@example.test"]));
    assert!(json["text"].as_str().unwrap().contains("482913"));
    assert!(json["text"].as_str().unwrap().contains("10 minutos"));
}

#[tokio::test]
async fn test_fr_con_011_a_provider_error_never_carries_the_provider_body() {
    let _budget = BUDGET.read().await;
    // covers: FR-CON-011
    let seen = Seen::default();
    let url = stub_provider(422, seen).await;
    let mailer = EmailBackend::resend(&url, SecretString::new("k"), "a@example.test").unwrap();
    let to = EmailAddress::parse("someone@example.test").unwrap();
    let error = mailer
        .send_code(&to, "123456", Duration::from_secs(600))
        .await
        .unwrap_err();
    assert_eq!(error, MailError::Rejected(422));
    let rendered = format!("{error} {error:?}");
    assert!(!rendered.contains("someone"), "{rendered}");
    assert!(!rendered.contains("example.test"), "{rendered}");
}

#[tokio::test]
async fn test_fr_con_011_the_file_provider_writes_the_message_to_its_directory_only() {
    let _budget = BUDGET.read().await;
    // covers: FR-CON-011 (dev and test provider)
    let dir = std::env::temp_dir().join(format!("gs-mail-{}", Uuid::new_v4().simple()));
    let mailer = EmailBackend::File {
        dir: dir.clone(),
        from: "GeoStream <no-reply@example.test>".to_string(),
    };
    let to = EmailAddress::parse("ana@example.test").unwrap();
    mailer
        .send_code(&to, "654321", Duration::from_secs(600))
        .await
        .unwrap();
    let files: Vec<_> = std::fs::read_dir(&dir).unwrap().collect();
    assert_eq!(files.len(), 1);
    let text = std::fs::read_to_string(files[0].as_ref().unwrap().path()).unwrap();
    assert!(text.contains("To: ana@example.test"));
    assert!(text.contains("654321"));
    std::fs::remove_dir_all(dir).unwrap();
}
