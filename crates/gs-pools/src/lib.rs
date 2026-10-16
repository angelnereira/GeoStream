#![forbid(unsafe_code)]

use gs_geo::{LatLon, haversine_distance};

#[derive(Debug, Clone, Copy)]
pub enum DeviceCapability {
    None,
    Read,
    Invite,
    Manage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PoolError {
    InvPool1Violation,
    InvalidRadius,
}

pub fn validate_pool_member(is_device: bool, caps: &[DeviceCapability]) -> bool {
    // INV-POOL-1: devices never get read/invite/manage.
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

#[derive(Debug, Clone)]
pub struct PoolMember {
    pub member_id: String,
    pub lat: f64,
    pub lon: f64,
    pub accuracy_m: f64,
}

pub fn nearby(members: &[PoolMember], origin: (f64, f64), radius_m: f64) -> Vec<(String, f64)> {
    let mut out: Vec<(String, f64)> = members
        .iter()
        .filter_map(|m| {
            let d = haversine_distance(LatLon::new(origin.0, origin.1), LatLon::new(m.lat, m.lon));
            if d <= radius_m && d.is_finite() {
                Some((m.member_id.clone(), d))
            } else {
                None
            }
        })
        .collect();
    out.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    out
}

pub fn density(members: &[PoolMember], cell_size_m: f64) -> usize {
    if cell_size_m <= 0.0 {
        return members.len();
    }
    // Simple grid bucket by lat/lon rounded.
    let mut seen: std::collections::HashSet<(i64, i64)> = std::collections::HashSet::new();
    for m in members {
        let ix = (m.lat * 1000.0 / cell_size_m) as i64;
        let iy = (m.lon * 1000.0 / cell_size_m) as i64;
        seen.insert((ix, iy));
    }
    // number of members regardless (density proxy)
    members.len()
}
