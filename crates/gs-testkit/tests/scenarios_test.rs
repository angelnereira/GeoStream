//! End-to-end scenario runner integration tests (docs/18 §6, contracts/README §5).
#![forbid(unsafe_code)]

use gs_domain::Subprotocol;
use gs_testkit::{parse_scenario, run_scenario};

#[tokio::test]
async fn test_scn_001_json() {
    // covers: FR-SES-001, FR-SHR-001, FR-SHR-002, FR-STR-002, FR-STR-003, FR-STR-004, FR-PRS-001
    let json_data = include_str!("../../../contracts/scenarios/scn_001.json");
    let scenario = parse_scenario(json_data).expect("valid scenario SCN-001");
    let report = run_scenario(&scenario, Subprotocol::Json)
        .await
        .expect("run SCN-001 JSON");
    assert_eq!(report.id, "SCN-001");
}

#[tokio::test]
async fn test_scn_001_proto() {
    // covers: FR-SES-001, FR-SHR-001, FR-SHR-002, FR-STR-002, FR-STR-003, FR-STR-004, FR-PRS-001
    let json_data = include_str!("../../../contracts/scenarios/scn_001.json");
    let scenario = parse_scenario(json_data).expect("valid scenario SCN-001");
    let report = run_scenario(&scenario, Subprotocol::Proto)
        .await
        .expect("run SCN-001 Proto");
    assert_eq!(report.id, "SCN-001");
}

#[tokio::test]
async fn test_scn_002_json() {
    let json_data = include_str!("../../../contracts/scenarios/scn_002.json");
    let scenario = parse_scenario(json_data).expect("valid scenario SCN-002");
    let report = run_scenario(&scenario, Subprotocol::Json)
        .await
        .expect("run SCN-002 JSON");
    assert_eq!(report.id, "SCN-002");
}

#[tokio::test]
async fn test_scn_002_proto() {
    let json_data = include_str!("../../../contracts/scenarios/scn_002.json");
    let scenario = parse_scenario(json_data).expect("valid scenario SCN-002");
    let report = run_scenario(&scenario, Subprotocol::Proto)
        .await
        .expect("run SCN-002 Proto");
    assert_eq!(report.id, "SCN-002");
}

#[tokio::test]
async fn test_scn_003_json() {
    let json_data = include_str!("../../../contracts/scenarios/scn_003.json");
    let scenario = parse_scenario(json_data).expect("valid scenario SCN-003");
    let report = run_scenario(&scenario, Subprotocol::Json)
        .await
        .expect("run SCN-003 JSON");
    assert_eq!(report.id, "SCN-003");
}

#[tokio::test]
async fn test_scn_003_proto() {
    let json_data = include_str!("../../../contracts/scenarios/scn_003.json");
    let scenario = parse_scenario(json_data).expect("valid scenario SCN-003");
    let report = run_scenario(&scenario, Subprotocol::Proto)
        .await
        .expect("run SCN-003 Proto");
    assert_eq!(report.id, "SCN-003");
}

#[tokio::test]
async fn test_scn_004_json() {
    let json_data = include_str!("../../../contracts/scenarios/scn_004.json");
    let scenario = parse_scenario(json_data).expect("valid scenario SCN-004");
    let report = run_scenario(&scenario, Subprotocol::Json)
        .await
        .expect("run SCN-004 JSON");
    assert_eq!(report.id, "SCN-004");
}

#[tokio::test]
async fn test_scn_004_proto() {
    let json_data = include_str!("../../../contracts/scenarios/scn_004.json");
    let scenario = parse_scenario(json_data).expect("valid scenario SCN-004");
    let report = run_scenario(&scenario, Subprotocol::Proto)
        .await
        .expect("run SCN-004 Proto");
    assert_eq!(report.id, "SCN-004");
}

#[tokio::test]
async fn test_scn_005_json() {
    let json_data = include_str!("../../../contracts/scenarios/scn_005.json");
    let scenario = parse_scenario(json_data).expect("valid scenario SCN-005");
    let report = run_scenario(&scenario, Subprotocol::Json)
        .await
        .expect("run SCN-005 JSON");
    assert_eq!(report.id, "SCN-005");
}

#[tokio::test]
async fn test_scn_005_proto() {
    let json_data = include_str!("../../../contracts/scenarios/scn_005.json");
    let scenario = parse_scenario(json_data).expect("valid scenario SCN-005");
    let report = run_scenario(&scenario, Subprotocol::Proto)
        .await
        .expect("run SCN-005 Proto");
    assert_eq!(report.id, "SCN-005");
}

#[tokio::test]
async fn test_scn_006_json() {
    let json_data = include_str!("../../../contracts/scenarios/scn_006.json");
    let scenario = parse_scenario(json_data).expect("valid scenario SCN-006");
    let report = run_scenario(&scenario, Subprotocol::Json)
        .await
        .expect("run SCN-006 JSON");
    assert_eq!(report.id, "SCN-006");
}

#[tokio::test]
async fn test_scn_006_proto() {
    let json_data = include_str!("../../../contracts/scenarios/scn_006.json");
    let scenario = parse_scenario(json_data).expect("valid scenario SCN-006");
    let report = run_scenario(&scenario, Subprotocol::Proto)
        .await
        .expect("run SCN-006 Proto");
    assert_eq!(report.id, "SCN-006");
}

#[tokio::test]
async fn test_scn_007_json() {
    let json_data = include_str!("../../../contracts/scenarios/scn_007.json");
    let scenario = parse_scenario(json_data).expect("valid scenario SCN-007");
    let report = run_scenario(&scenario, Subprotocol::Json)
        .await
        .expect("run SCN-007 JSON");
    assert_eq!(report.id, "SCN-007");
}

#[tokio::test]
async fn test_scn_007_proto() {
    let json_data = include_str!("../../../contracts/scenarios/scn_007.json");
    let scenario = parse_scenario(json_data).expect("valid scenario SCN-007");
    let report = run_scenario(&scenario, Subprotocol::Proto)
        .await
        .expect("run SCN-007 Proto");
    assert_eq!(report.id, "SCN-007");
}

#[tokio::test]
async fn test_scn_008_json() {
    let json_data = include_str!("../../../contracts/scenarios/scn_008.json");
    let scenario = parse_scenario(json_data).expect("valid scenario SCN-008");
    let report = run_scenario(&scenario, Subprotocol::Json)
        .await
        .expect("run SCN-008 JSON");
    assert_eq!(report.id, "SCN-008");
}

#[tokio::test]
async fn test_scn_008_proto() {
    let json_data = include_str!("../../../contracts/scenarios/scn_008.json");
    let scenario = parse_scenario(json_data).expect("valid scenario SCN-008");
    let report = run_scenario(&scenario, Subprotocol::Proto)
        .await
        .expect("run SCN-008 Proto");
    assert_eq!(report.id, "SCN-008");
}

#[tokio::test]
#[ignore = "slow/nightly"]
async fn test_scn_009_json() {
    let json_data = include_str!("../../../contracts/scenarios/scn_009.json");
    let scenario = parse_scenario(json_data).expect("valid scenario SCN-009");
    let report = run_scenario(&scenario, Subprotocol::Json)
        .await
        .expect("run SCN-009 JSON");
    assert_eq!(report.id, "SCN-009");
}

#[tokio::test]
#[ignore = "slow/nightly"]
async fn test_scn_009_proto() {
    let json_data = include_str!("../../../contracts/scenarios/scn_009.json");
    let scenario = parse_scenario(json_data).expect("valid scenario SCN-009");
    let report = run_scenario(&scenario, Subprotocol::Proto)
        .await
        .expect("run SCN-009 Proto");
    assert_eq!(report.id, "SCN-009");
}

#[tokio::test]
async fn test_scn_010_json() {
    let json_data = include_str!("../../../contracts/scenarios/scn_010.json");
    let scenario = parse_scenario(json_data).expect("valid scenario SCN-010");
    let report = run_scenario(&scenario, Subprotocol::Json)
        .await
        .expect("run SCN-010 JSON");
    assert_eq!(report.id, "SCN-010");
}

#[tokio::test]
async fn test_scn_010_proto() {
    let json_data = include_str!("../../../contracts/scenarios/scn_010.json");
    let scenario = parse_scenario(json_data).expect("valid scenario SCN-010");
    let report = run_scenario(&scenario, Subprotocol::Proto)
        .await
        .expect("run SCN-010 Proto");
    assert_eq!(report.id, "SCN-010");
}
