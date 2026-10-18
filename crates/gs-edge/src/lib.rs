#![forbid(unsafe_code)]

use hmac::digest::Update;
use hmac::{KeyInit, Mac};
use sha2::Sha256;

type HmacSha256 = hmac::SimpleHmac<Sha256>;

/// Verifica firma HMAC-SHA256 con tiempo constante (comparacion via Mac::verify_slice).
/// Devuelve false ante clave vacía o trama malformada — sin panico y sin filtrar.
pub fn verify_lpwam_signature(secret: &[u8], message: &[u8], signature: &[u8]) -> bool {
    if secret.is_empty() {
        return false;
    }
    let mut mac = match HmacSha256::new_from_slice(secret) {
        Ok(m) => m,
        Err(_) => return false,
    };
    Update::update(&mut mac, message);
    mac.verify_slice(signature).is_ok()
}

/// Ephemeral identifier rotation (E8 §2.3): derived from EIK + counter + window,
/// only the tag knows EIK; server stores only the ephemeral id.
pub fn ephemeral_id(eik: &[u8], counter: u64, window: u64) -> u64 {
    use sha2::Digest;
    let mut h = Sha256::new();
    Digest::update(&mut h, eik);
    Digest::update(&mut h, counter.to_le_bytes());
    Digest::update(&mut h, window.to_le_bytes());
    let digest = h.finalize();
    u64::from_le_bytes(digest[0..8].try_into().unwrap_or([0u8; 8]))
}

pub fn is_separated(last_seen_ms: u64, now_ms: u64, timeout_ms: u64) -> bool {
    now_ms.saturating_sub(last_seen_ms) > timeout_ms
}

/// Rotation state of a tag (E8, `25` §2.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TagState {
    Active,
    Separated,
    Removed,
}

/// Default EID rotation period in seconds (`edge_rotation_s`, `17` §2.10).
pub const EDGE_ROTATION_S: u64 = 1024;
/// Rotation period once the tag is `SEPARATED` (`25` §3): deliberately slow so
/// that removing the DULT protection is noticeable.
pub const EDGE_SEPARATED_ROTATION_S: u64 = 86_400;

/// How often the ephemeral identifier rotates, by state.
#[must_use]
pub fn rotation_period_s(state: TagState) -> u64 {
    match state {
        TagState::Active => EDGE_ROTATION_S,
        TagState::Separated | TagState::Removed => EDGE_SEPARATED_ROTATION_S,
    }
}

/// DULT thresholds (`17` §2.10).
#[derive(Debug, Clone, Copy)]
pub struct DultThresholds {
    pub min_sightings: u32,
    pub min_distinct_locations: u32,
    pub min_distance_m: f64,
    pub min_duration_min: u32,
}

impl Default for DultThresholds {
    fn default() -> Self {
        Self {
            min_sightings: 3,
            min_distinct_locations: 3,
            min_distance_m: 500.0,
            min_duration_min: 60,
        }
    }
}

/// Whether an unwanted-tracker alert fires. **All four** thresholds must be met.
///
/// The alert never carries coordinates (`25` §3): telling the victim the route
/// would hand the result to the tracker as soon as the device leaks.
#[must_use]
pub fn dult_alert(
    thresholds: &DultThresholds,
    distinct_eids: u32,
    distinct_locations: u32,
    distance_m: f64,
    duration_min: u32,
) -> bool {
    distinct_eids >= thresholds.min_sightings
        && distinct_locations >= thresholds.min_distinct_locations
        && distance_m >= thresholds.min_distance_m
        && duration_min >= thresholds.min_duration_min
}

/// Whether an LPWAN timestamp is inside the accepted window.
#[must_use]
pub fn timestamp_within_window(skew_s: i64, max_skew_s: i64) -> bool {
    skew_s.abs() <= max_skew_s
}

#[cfg(test)]
mod edge_tests {
    use super::*;

    #[test]
    fn edge_eid_003_separated_rotates_slowly() {
        assert_eq!(rotation_period_s(TagState::Active), 1024);
        assert_eq!(rotation_period_s(TagState::Separated), 86_400);
    }

    #[test]
    fn edge_eid_001_ephemeral_ids_are_distinct_per_rotation() {
        let eik = b"tag-secret-key";
        let ids: std::collections::HashSet<u64> = (0..5).map(|i| ephemeral_id(eik, i, i)).collect();
        assert_eq!(ids.len(), 5, "each rotation must produce a new EID");
    }

    #[test]
    fn edge_eid_002_eid_is_not_derivable_from_a_stable_identifier() {
        // Two different EIKs never collide on the same counter, and the same EIK
        // on different counters never repeats.
        let a = ephemeral_id(b"eik-a", 1, 1);
        let b = ephemeral_id(b"eik-b", 1, 1);
        assert_ne!(a, b);
        assert_ne!(ephemeral_id(b"eik-a", 1, 1), ephemeral_id(b"eik-a", 2, 1));
    }

    #[test]
    fn edge_dult_001_all_four_thresholds_fire() {
        let t = DultThresholds::default();
        assert!(dult_alert(&t, 3, 3, 1200.0, 90));
    }

    #[test]
    fn edge_dult_002_short_distance_does_not_fire() {
        let t = DultThresholds::default();
        assert!(!dult_alert(&t, 3, 3, 100.0, 90));
    }

    #[test]
    fn edge_dult_003_short_duration_does_not_fire() {
        let t = DultThresholds::default();
        assert!(!dult_alert(&t, 3, 3, 1200.0, 10));
    }

    #[test]
    fn edge_lpw_004_unknown_model_is_rejected_never_best_effort() {
        assert_eq!(
            decode_position("desconocido-9999", 1, &[0u8; 8]),
            Err(DecodeError::UnknownModel)
        );
    }

    #[test]
    fn reference_decoder_round_trips_a_position() {
        let mut payload = Vec::new();
        payload.extend_from_slice(&89_824_000i32.to_be_bytes()); // 8.9824
        payload.extend_from_slice(&(-795_199_000i32).to_be_bytes()); // -79.5199
        let fixes = decode_position("lora-sensor-a", 1, &payload).expect("decode");
        assert_eq!(fixes.len(), 1);
        assert!((fixes[0].lat - 8.9824).abs() < 1e-6);
        assert!((fixes[0].lon - (-79.5199)).abs() < 1e-6);
    }

    #[test]
    fn malformed_payload_is_rejected() {
        assert_eq!(
            decode_position("lora-sensor-a", 1, &[1, 2, 3]),
            Err(DecodeError::Malformed)
        );
        assert_eq!(
            decode_position("lora-sensor-a", 1, &[]),
            Err(DecodeError::Malformed)
        );
    }

    #[test]
    fn edge_lpw_003_timestamp_outside_the_window_is_rejected() {
        assert!(!timestamp_within_window(3600, 300));
        assert!(timestamp_within_window(60, 300));
        assert!(timestamp_within_window(-60, 300));
    }
}

/// One position decoded from an LPWAN payload.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RawFix {
    pub lat: f64,
    pub lon: f64,
}

/// Why a payload could not be decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeError {
    /// No decoder is registered for the model. **Never** a best-effort guess
    /// (`EDGE-LPW-004`): a wrong decode produces invented coordinates that look
    /// like data, which is worse than no data.
    UnknownModel,
    /// The decoder is registered but the payload does not match its format.
    Malformed,
    /// The decoded position is outside the WGS 84 domain.
    OutOfRange,
}

/// A per-model LPWAN decoder (`docs/25` §4).
pub trait LpwanDecoder {
    /// The model name this decoder answers to.
    fn model(&self) -> &'static str;
    /// Decodes a frame. `fport` is the LoRaWAN application port.
    fn decode(&self, fport: u8, payload: &[u8]) -> Result<Vec<RawFix>, DecodeError>;
}

/// Reference decoder for `lora-sensor-a`: one big-endian frame of
/// `int32 lat_e7` + `int32 lon_e7`, optionally repeated for a batch.
///
/// It exists to have a decoder to test the registry against, not to claim a
/// real device's format: `20` §9 requires verifying each network server's format
/// before implementing it.
pub struct LoraSensorA;

impl LpwanDecoder for LoraSensorA {
    fn model(&self) -> &'static str {
        "lora-sensor-a"
    }

    fn decode(&self, _fport: u8, payload: &[u8]) -> Result<Vec<RawFix>, DecodeError> {
        if payload.is_empty() || !payload.len().is_multiple_of(8) {
            return Err(DecodeError::Malformed);
        }
        let mut out = Vec::with_capacity(payload.len() / 8);
        for chunk in payload.chunks(8) {
            let lat = i32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]) as f64 / 1e7;
            let lon = i32::from_be_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]) as f64 / 1e7;
            if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
                return Err(DecodeError::OutOfRange);
            }
            out.push(RawFix { lat, lon });
        }
        Ok(out)
    }
}

/// The registered decoders. Deliberately a short, explicit list: a model without
/// a decoder is `422`, never a best-effort decode.
#[must_use]
pub fn decoder_for(model: &str) -> Option<&'static dyn LpwanDecoder> {
    static LORA_SENSOR_A: LoraSensorA = LoraSensorA;
    match model {
        "lora-sensor-a" => Some(&LORA_SENSOR_A),
        _ => None,
    }
}

/// Decodes a frame for a model, or reports why not.
pub fn decode_position(model: &str, fport: u8, payload: &[u8]) -> Result<Vec<RawFix>, DecodeError> {
    match decoder_for(model) {
        Some(decoder) => decoder.decode(fport, payload),
        None => Err(DecodeError::UnknownModel),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hmac_roundtrip() {
        let secret = b"truly-secret";
        let msg = b"hello lpwan";
        let mut mac = HmacSha256::new_from_slice(secret).unwrap();
        Update::update(&mut mac, msg);
        let sig = mac.finalize().into_bytes().to_vec();
        assert!(verify_lpwam_signature(secret, msg, &sig));
        assert!(!verify_lpwam_signature(secret, b"tampered", &sig));
        assert!(!verify_lpwam_signature(b"", msg, &sig));
    }
    #[test]
    fn separated_detection() {
        assert!(!is_separated(1000, 1500, 1000));
        assert!(is_separated(1000, 2500, 1000));
    }
}
