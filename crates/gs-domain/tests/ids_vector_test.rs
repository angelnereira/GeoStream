//! Test vector validation for typed entity identifiers.
//! Normative reference: contracts/test-vectors/ids.json and docs/05_DOMAIN_MODEL.md §2.
#![forbid(unsafe_code)]

use gs_domain::*;
use serde::Deserialize;
use std::fs;
use std::path::PathBuf;
use std::str::FromStr;

#[derive(Deserialize)]
struct IdVectorFile {
    valid: Vec<ValidVector>,
    invalid: Vec<InvalidVector>,
    external_subject_id: ExternalSubjectVectors,
    installation_id: InstallationVectors,
}

#[derive(Deserialize)]
struct ValidVector {
    value: String,
    #[serde(rename = "type")]
    type_name: String,
}

#[derive(Deserialize)]
struct InvalidVector {
    value: String,
    #[serde(rename = "type")]
    type_name: String,
    reason: String,
}

#[derive(Deserialize)]
struct ExternalSubjectVectors {
    valid: Vec<String>,
    invalid: Vec<String>,
}

#[derive(Deserialize)]
struct InstallationVectors {
    valid: Vec<String>,
    invalid: Vec<String>,
}

fn load_vectors() -> IdVectorFile {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir.parent().unwrap().parent().unwrap();
    let vector_path = repo_root.join("contracts/test-vectors/ids.json");
    let content = fs::read_to_string(&vector_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", vector_path.display()));
    serde_json::from_str(&content).expect("failed to parse ids.json")
}

#[test]
fn test_valid_prefixed_ids_vectors() {
    let vectors = load_vectors();
    for v in &vectors.valid {
        match v.type_name.as_str() {
            "SessionId" => {
                let id = SessionId::from_str(&v.value).expect("valid SessionId failed to parse");
                assert_eq!(id.to_string(), v.value);
                let json = serde_json::to_string(&id).unwrap();
                let deserialized: SessionId = serde_json::from_str(&json).unwrap();
                assert_eq!(id, deserialized);
            }
            "MemberId" => {
                let id = MemberId::from_str(&v.value).expect("valid MemberId failed to parse");
                assert_eq!(id.to_string(), v.value);
            }
            "SubjectId" => {
                let id = SubjectId::from_str(&v.value).expect("valid SubjectId failed to parse");
                assert_eq!(id.to_string(), v.value);
            }
            "DeviceId" => {
                let id = DeviceId::from_str(&v.value).expect("valid DeviceId failed to parse");
                assert_eq!(id.to_string(), v.value);
            }
            "TenantId" => {
                let id = TenantId::from_str(&v.value).expect("valid TenantId failed to parse");
                assert_eq!(id.to_string(), v.value);
            }
            other => panic!("unknown type {other} in valid vectors"),
        }
    }
}

#[test]
fn test_invalid_prefixed_ids_vectors() {
    let vectors = load_vectors();
    for v in &vectors.invalid {
        match v.type_name.as_str() {
            "SessionId" => {
                let res = SessionId::from_str(&v.value);
                assert!(
                    res.is_err(),
                    "expected invalid SessionId for '{}' (reason: {})",
                    v.value,
                    v.reason
                );
            }
            other => panic!("unknown type {other} in invalid vectors"),
        }
    }
}

#[test]
fn test_external_subject_id_vectors() {
    let vectors = load_vectors();
    for val in &vectors.external_subject_id.valid {
        let parsed = ExternalSubjectId::from_str(val).expect("valid external_subject_id failed");
        assert_eq!(parsed.as_str(), val.as_str());
        let json = serde_json::to_string(&parsed).unwrap();
        let deserialized: ExternalSubjectId = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, deserialized);
    }

    for val in &vectors.external_subject_id.invalid {
        let res = ExternalSubjectId::from_str(val);
        assert!(
            res.is_err(),
            "expected invalid ExternalSubjectId for '{val}'"
        );
    }
}

#[test]
fn test_installation_id_vectors() {
    let vectors = load_vectors();
    for val in &vectors.installation_id.valid {
        let parsed = InstallationId::from_str(val).expect("valid installation_id failed");
        assert_eq!(parsed.to_string(), *val);
        let json = serde_json::to_string(&parsed).unwrap();
        let deserialized: InstallationId = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, deserialized);
    }

    for val in &vectors.installation_id.invalid {
        let res = InstallationId::from_str(val);
        assert!(res.is_err(), "expected invalid InstallationId for '{val}'");
    }
}

#[test]
fn test_all_14_entity_id_prefixes() {
    let ten = TenantId::new();
    assert!(ten.to_string().starts_with("ten_"));
    let app = ApplicationId::new();
    assert!(app.to_string().starts_with("app_"));
    let key = ApiKeyId::new();
    assert!(key.to_string().starts_with("key_"));
    let sub = SubjectId::new();
    assert!(sub.to_string().starts_with("sub_"));
    let dev = DeviceId::new();
    assert!(dev.to_string().starts_with("dev_"));
    let ses = SessionId::new();
    assert!(ses.to_string().starts_with("ses_"));
    let mem = MemberId::new();
    assert!(mem.to_string().starts_with("mem_"));
    let inv = InvitationId::new();
    assert!(inv.to_string().starts_with("inv_"));
    let whk = WebhookEndpointId::new();
    assert!(whk.to_string().starts_with("whk_"));
    let evt = EventId::new();
    assert!(evt.to_string().starts_with("evt_"));
    let geo = GeofenceId::new();
    assert!(geo.to_string().starts_with("geo_"));
    let con = ConnectionId::new();
    assert!(con.to_string().starts_with("con_"));
    let req = RequestId::new();
    assert!(req.to_string().starts_with("req_"));
    let aud = AuditId::new();
    assert!(aud.to_string().starts_with("aud_"));

    // Verify roundtrip for each
    assert_eq!(TenantId::from_str(&ten.to_string()).unwrap(), ten);
    assert_eq!(ApplicationId::from_str(&app.to_string()).unwrap(), app);
    assert_eq!(ApiKeyId::from_str(&key.to_string()).unwrap(), key);
    assert_eq!(SubjectId::from_str(&sub.to_string()).unwrap(), sub);
    assert_eq!(DeviceId::from_str(&dev.to_string()).unwrap(), dev);
    assert_eq!(SessionId::from_str(&ses.to_string()).unwrap(), ses);
    assert_eq!(MemberId::from_str(&mem.to_string()).unwrap(), mem);
    assert_eq!(InvitationId::from_str(&inv.to_string()).unwrap(), inv);
    assert_eq!(WebhookEndpointId::from_str(&whk.to_string()).unwrap(), whk);
    assert_eq!(EventId::from_str(&evt.to_string()).unwrap(), evt);
    assert_eq!(GeofenceId::from_str(&geo.to_string()).unwrap(), geo);
    assert_eq!(ConnectionId::from_str(&con.to_string()).unwrap(), con);
    assert_eq!(RequestId::from_str(&req.to_string()).unwrap(), req);
    assert_eq!(AuditId::from_str(&aud.to_string()).unwrap(), aud);

    // Cross-prefix rejection
    assert!(SessionId::from_str(&ten.to_string()).is_err());
    assert!(TenantId::from_str(&mem.to_string()).is_err());
}
