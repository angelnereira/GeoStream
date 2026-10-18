#![forbid(unsafe_code)]

use gs_geo::{LatLon, haversine_distance};

/// Capabilities a pool member can hold. `INV-POOL-1`: a **device** member can
/// never hold `read`, `invite` or `manage`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceCapability {
    None,
    Read,
    Invite,
    Manage,
}

/// Whether the pairing of a member with capabilities is legal (`INV-POOL-1`).
#[must_use]
pub fn validate_pool_member(is_device: bool, caps: &[DeviceCapability]) -> bool {
    if is_device {
        for c in caps {
            if matches!(
                c,
                DeviceCapability::Read | DeviceCapability::Invite | DeviceCapability::Manage
            ) {
                return false;
            }
        }
    }
    true
}

/// Availability projected to other members (`23` §1.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Availability {
    Available,
    Busy,
    Paused,
    Unspecified,
}

/// One member of the live pool index.
#[derive(Debug, Clone)]
pub struct PoolMember {
    pub member_id: String,
    pub lat: f64,
    pub lon: f64,
    pub accuracy_m: f64,
    pub availability: Availability,
    pub attributes: Vec<(String, String)>,
    /// Whether the member has a live snapshot at all.
    pub connected: bool,
    /// Age of the last sample in ms.
    pub age_ms: u64,
}

/// Ranking mode of `nearby`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rank {
    Distance,
    Eta,
}

/// Query parameters of `nearby` (`23` §3.1).
#[derive(Debug, Clone)]
pub struct NearbyParams {
    pub lat: f64,
    pub lon: f64,
    pub radius_m: f64,
    pub limit: usize,
    pub rank: Rank,
    /// `max_age_s`: 0 admits only the current window; a positive value is an
    /// explicit decision by the integrator to accept older samples.
    pub max_age_s: u64,
    /// Equality filter on attributes (all pairs must match).
    pub attributes: Vec<(String, String)>,
    /// Mode speed for `rank = eta`.
    pub eta_speed_mps: f64,
    pub eta_circuity: f64,
}

/// One member of the `nearby` response.
#[derive(Debug, Clone)]
pub struct NearbyMember {
    pub member_id: String,
    pub distance_m: f64,
    pub age_ms: u64,
    pub availability: Availability,
    pub attributes: Vec<(String, String)>,
    pub lat: f64,
    pub lon: f64,
    pub eta_s: Option<f64>,
    pub mode: Option<&'static str>,
}

/// Default window (seconds) that `max_age_s = 0` admits: the snapshot TTL.
pub const DEFAULT_FRESH_WINDOW_S: u64 = 120;

fn candidate(member: &PoolMember, params: &NearbyParams) -> bool {
    // PAUSED is online but not an option: never offered for assignment.
    if member.availability == Availability::Paused {
        return false;
    }
    if !member.connected {
        return false;
    }
    let window_s = if params.max_age_s == 0 {
        DEFAULT_FRESH_WINDOW_S
    } else {
        params.max_age_s
    };
    if member.age_ms > window_s.saturating_mul(1000) {
        return false;
    }
    params
        .attributes
        .iter()
        .all(|(k, v)| member.attributes.iter().any(|(mk, mv)| mk == k && mv == v))
}

/// `nearby` (`23` §3.1). Excludes `PAUSED` and disconnected members unless
/// `max_age_s` explicitly admits them; filters by attributes; ranks by distance
/// or by ETA.
#[must_use]
pub fn nearby(members: &[PoolMember], params: &NearbyParams) -> Vec<NearbyMember> {
    let center = LatLon::new(params.lat, params.lon);
    let mut out: Vec<NearbyMember> = members
        .iter()
        .filter(|m| candidate(m, params))
        .filter_map(|m| {
            let distance = haversine_distance(center, LatLon::new(m.lat, m.lon));
            if distance > params.radius_m {
                return None;
            }
            Some(NearbyMember {
                member_id: m.member_id.clone(),
                distance_m: distance,
                age_ms: m.age_ms,
                availability: m.availability,
                attributes: m.attributes.clone(),
                lat: m.lat,
                lon: m.lon,
                eta_s: None,
                mode: None,
            })
        })
        .collect();

    match params.rank {
        Rank::Distance => out.sort_by(|a, b| a.distance_m.total_cmp(&b.distance_m)),
        Rank::Eta => {
            for m in &mut out {
                m.eta_s = Some(m.distance_m * params.eta_circuity / params.eta_speed_mps.max(1e-6));
                m.mode = Some("DRIVING");
            }
            out.sort_by(|a, b| {
                a.eta_s
                    .unwrap_or(f64::MAX)
                    .total_cmp(&b.eta_s.unwrap_or(f64::MAX))
            });
        }
    }

    if out.len() > params.limit {
        out.truncate(params.limit);
    }
    out
}

/// One ring of the `rings` response.
#[derive(Debug, Clone)]
pub struct Ring {
    pub ring: u32,
    pub inner_radius_m: f64,
    pub outer_radius_m: f64,
    pub members: Vec<NearbyMember>,
    pub truncated: bool,
}

/// `rings` (`23` §3.2). Exactly one of `ring_step_m` / `k` must be given; the
/// caller validates that (422 otherwise).
#[must_use]
pub fn rings(
    members: &[PoolMember],
    params: &NearbyParams,
    ring_step_m: Option<f64>,
    k: Option<u32>,
    per_ring_limit: usize,
) -> Vec<Ring> {
    let candidates = nearby(members, params);
    // The outermost ring that holds a member defines the count when using
    // `ring_step_m`; with `k` the count is fixed.
    let step = match (ring_step_m, k) {
        (Some(step), None) => step.max(1.0),
        (None, Some(k)) => {
            let max_d = candidates
                .iter()
                .map(|m| m.distance_m)
                .fold(0.0_f64, f64::max);
            (max_d / k.max(1) as f64).max(1.0)
        }
        _ => 1.0,
    };
    let n_rings = match (ring_step_m, k) {
        (Some(_), None) => {
            let max_d = candidates
                .iter()
                .map(|m| m.distance_m)
                .fold(0.0_f64, f64::max);
            (max_d / step).ceil() as u32
        }
        (None, Some(k)) => k,
        _ => 0,
    };

    let mut out = Vec::new();
    for r in 0..n_rings {
        let inner = r as f64 * step;
        let outer = (r as f64 + 1.0) * step;
        let mut in_ring: Vec<NearbyMember> = candidates
            .iter()
            .filter(|m| {
                // The innermost ring includes the centre itself (distance 0).
                let lower_ok = if r == 0 {
                    m.distance_m >= inner
                } else {
                    m.distance_m > inner
                };
                lower_ok && m.distance_m <= outer
            })
            .cloned()
            .collect();
        let truncated = in_ring.len() > per_ring_limit;
        if truncated {
            in_ring.truncate(per_ring_limit);
        }
        out.push(Ring {
            ring: r,
            inner_radius_m: inner,
            outer_radius_m: outer,
            members: in_ring,
            truncated,
        });
    }
    out
}

/// One density marker (`23` §3.3): anonymous, displaced centroid.
#[derive(Debug, Clone)]
pub struct DensityMarker {
    pub lat: f64,
    pub lon: f64,
    pub count: u32,
    pub resolution: u8,
    pub heading_sector: Option<u8>,
    pub delay_ms: u64,
}

/// `density` (`23` §3.3). The centroid is displaced deterministically within a
/// bounded radius and the heading is quantised to 8 sectors; **no identifier and
/// no exact timestamp** is ever present.
#[must_use]
pub fn density(
    members: &[PoolMember],
    params: &NearbyParams,
    resolution: u8,
    max_markers: usize,
    delay_s: u64,
) -> Vec<DensityMarker> {
    let center = LatLon::new(params.lat, params.lon);
    // Group candidates into a coarse grid; each cell becomes one marker.
    let cell_deg = 0.01_f64.max(params.radius_m / 111_195.0 / 20.0);
    let mut cells: std::collections::BTreeMap<(i64, i64), (f64, f64, u32)> =
        std::collections::BTreeMap::new();
    for m in members.iter().filter(|m| candidate(m, params)) {
        let d = haversine_distance(center, LatLon::new(m.lat, m.lon));
        if d > params.radius_m {
            continue;
        }
        let key = ((m.lat / cell_deg) as i64, (m.lon / cell_deg) as i64);
        let e = cells.entry(key).or_insert((0.0, 0.0, 0));
        e.0 += m.lat;
        e.1 += m.lon;
        e.2 += 1;
    }

    let mut markers: Vec<DensityMarker> = cells
        .into_iter()
        .map(|(key, (sum_lat, sum_lon, count))| {
            let centroid_lat = sum_lat / count as f64;
            let centroid_lon = sum_lon / count as f64;
            // Deterministic displacement within one tenth of the cell.
            let (dx, dy) = displacement(key.0, key.1, cell_deg);
            let heading = Some((hash(key.0, key.1) % 8) as u8);
            DensityMarker {
                lat: centroid_lat + dy,
                lon: centroid_lon + dx,
                count,
                resolution,
                heading_sector: heading,
                delay_ms: delay_s.saturating_mul(1000),
            }
        })
        .collect();

    markers.sort_by_key(|m| std::cmp::Reverse(m.count));
    if markers.len() > max_markers {
        markers.truncate(max_markers);
    }
    markers
}

fn hash(a: i64, b: i64) -> u64 {
    let mut h = 0xcbf29ce484222325_u64;
    for v in [a, b] {
        h ^= v as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// Deterministic displacement in degrees, magnitude in `[0, cell/10)`.
fn displacement(a: i64, b: i64, cell_deg: f64) -> (f64, f64) {
    let h = hash(a, b);
    let angle = (h % 360) as f64 * std::f64::consts::PI / 180.0;
    let mag = (h >> 9) % 1000;
    let radius_deg = cell_deg * 0.1 * (mag as f64 / 1000.0);
    (radius_deg * angle.cos(), radius_deg * angle.sin())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Members from `contracts/test-vectors/pool_queries.json`.
    fn synthetic() -> Vec<PoolMember> {
        let attr = |pairs: &[(&str, &str)]| {
            pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect()
        };
        vec![
            PoolMember {
                member_id: "near".into(),
                lat: 8.9824,
                lon: -79.5199,
                accuracy_m: 8.0,
                availability: Availability::Available,
                attributes: attr(&[("vehicle_type", "van"), ("seats", "4")]),
                connected: true,
                age_ms: 0,
            },
            PoolMember {
                member_id: "mid".into(),
                lat: 8.9870,
                lon: -79.5199,
                accuracy_m: 8.0,
                availability: Availability::Busy,
                attributes: attr(&[("vehicle_type", "car"), ("seats", "4")]),
                connected: true,
                age_ms: 0,
            },
            PoolMember {
                member_id: "far".into(),
                lat: 9.0050,
                lon: -79.5199,
                accuracy_m: 8.0,
                availability: Availability::Paused,
                attributes: attr(&[("vehicle_type", "car")]),
                connected: true,
                age_ms: 0,
            },
            PoolMember {
                member_id: "paused-far".into(),
                lat: 8.9840,
                lon: -79.5000,
                accuracy_m: 8.0,
                availability: Availability::Paused,
                attributes: attr(&[]),
                connected: true,
                age_ms: 0,
            },
            PoolMember {
                member_id: "offline".into(),
                lat: 8.9830,
                lon: -79.5190,
                accuracy_m: 8.0,
                availability: Availability::Available,
                attributes: attr(&[]),
                connected: false,
                age_ms: 0,
            },
        ]
    }

    fn params(radius_m: f64, rank: Rank) -> NearbyParams {
        NearbyParams {
            lat: 8.9824,
            lon: -79.5199,
            radius_m,
            limit: 20,
            rank,
            max_age_s: 0,
            attributes: Vec::new(),
            eta_speed_mps: 6.9,
            eta_circuity: 1.35,
        }
    }

    #[test]
    fn pool_near_001_small_radius() {
        let out = nearby(&synthetic(), &params(200.0, Rank::Distance));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].member_id, "near");
    }

    #[test]
    fn pool_near_002_excludes_paused_and_offline() {
        let out = nearby(&synthetic(), &params(5000.0, Rank::Distance));
        let ids: Vec<&str> = out.iter().map(|m| m.member_id.as_str()).collect();
        assert_eq!(ids, vec!["near", "mid"]);
    }

    #[test]
    fn pool_near_003_attribute_filter() {
        let mut p = params(5000.0, Rank::Distance);
        p.attributes = vec![("vehicle_type".to_string(), "van".to_string())];
        let out = nearby(&synthetic(), &p);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].member_id, "near");
    }

    #[test]
    fn pool_near_004_rank_eta_orders_and_sets_mode() {
        let out = nearby(&synthetic(), &params(5000.0, Rank::Eta));
        assert_eq!(out.len(), 2);
        assert!(
            out.iter()
                .all(|m| m.eta_s.is_some() && m.mode == Some("DRIVING"))
        );
        assert!(out[0].eta_s.unwrap() <= out[1].eta_s.unwrap());
    }

    #[test]
    fn pool_near_005_max_age_admits_old_but_not_absent() {
        let mut p = params(5000.0, Rank::Distance);
        p.max_age_s = 300;
        let out = nearby(&synthetic(), &p);
        // The offline member has no sample at all, so it stays out.
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn pool_ring_001_two_rings() {
        let out = rings(
            &synthetic(),
            &params(5000.0, Rank::Distance),
            Some(500.0),
            None,
            20,
        );
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].members.len(), 1);
        assert_eq!(out[1].members.len(), 1);
        assert!(!out.iter().any(|r| r.truncated));
    }

    #[test]
    fn pool_ring_002_truncates_and_marks() {
        // Two members in the same ring with `per_ring_limit = 1` force truncation.
        let mut members = synthetic();
        let mut twin = members[0].clone();
        twin.member_id = "near-twin".into();
        members.push(twin);
        let out = rings(&members, &params(5000.0, Rank::Distance), None, Some(3), 1);
        assert_eq!(out.len(), 3, "k = 3 asks for three rings");
        assert!(
            out.iter().any(|r| r.truncated),
            "a ring over per_ring_limit must be marked truncated"
        );
    }

    #[test]
    fn density_has_no_identifiers_and_bounded_markers() {
        let out = density(&synthetic(), &params(20000.0, Rank::Distance), 9, 15, 10);
        assert!(!out.is_empty());
        assert!(out.len() <= 15);
        for m in &out {
            assert!(m.count >= 1);
            assert!(m.heading_sector.is_none_or(|h| h < 8));
            assert!(m.delay_ms >= 10_000);
        }
    }

    #[test]
    fn density_centroid_is_displaced_from_any_member() {
        let members = synthetic();
        let out = density(&members, &params(20000.0, Rank::Distance), 9, 15, 10);
        for marker in &out {
            for m in &members {
                let d = haversine_distance(
                    LatLon::new(marker.lat, marker.lon),
                    LatLon::new(m.lat, m.lon),
                );
                // The marker must not coincide exactly with a real member.
                assert!(d > 0.01, "marker coincides with member {}", m.member_id);
            }
        }
    }

    #[test]
    fn inv_pool_1_devices_cannot_hold_read_invite_manage() {
        assert!(validate_pool_member(true, &[DeviceCapability::None]));
        assert!(!validate_pool_member(true, &[DeviceCapability::Read]));
        assert!(!validate_pool_member(true, &[DeviceCapability::Invite]));
        assert!(!validate_pool_member(true, &[DeviceCapability::Manage]));
        // Applications may hold anything.
        assert!(validate_pool_member(false, &[DeviceCapability::Manage]));
    }
}
