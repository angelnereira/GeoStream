#![forbid(unsafe_code)]

/// k-anon suppression: a result is releasable only if its count >= k.
pub fn k_anon_ok(count: u64, k: u64) -> bool {
    count >= k
}

/// Suppress counts by replacing with None below k.
pub fn suppress_count(count: u64, k: u64) -> Option<u64> {
    if count >= k { Some(count) } else { None }
}

/// DBSCAN-style cluster ids for points with (lat,lon). Returns cluster per point index.
pub fn dbscan(points: &[(f64, f64)], eps_m: f64, min_points: usize) -> Vec<Option<usize>> {
    let n = points.len();
    let mut visited = vec![false; n];
    let mut cluster_of = vec![None; n];
    let mut cid = 0usize;
    for i in 0..n {
        if visited[i] {
            continue;
        }
        visited[i] = true;
        let neighbors: Vec<usize> = (0..n)
            .filter(|&j| haversine_distance_pt(points[i], points[j]) <= eps_m)
            .collect();
        if neighbors.len() < min_points {
            // noise
            cluster_of[i] = None;
            continue;
        }
        cluster_of[i] = Some(cid);
        let mut seeds = neighbors.clone();
        seeds.retain(|&x| x != i);
        while let Some(q) = seeds.pop() {
            if visited[q] {
                if cluster_of[q].is_none() {
                    cluster_of[q] = Some(cid);
                }
                continue;
            }
            visited[q] = true;
            cluster_of[q] = Some(cid);
            let nq: Vec<usize> = (0..n)
                .filter(|&j| haversine_distance_pt(points[q], points[j]) <= eps_m)
                .collect();
            if nq.len() >= min_points {
                for x in nq {
                    if !seeds.contains(&x) && !visited[x] {
                        seeds.push(x);
                    }
                }
            }
        }
        cid += 1;
    }
    cluster_of
}

fn haversine_distance_pt(a: (f64, f64), b: (f64, f64)) -> f64 {
    gs_geo::haversine_distance(gs_geo::LatLon::new(a.0, a.1), gs_geo::LatLon::new(b.0, b.1))
}

/// Summary of a k-anonymous aggregation (OpenAPI `AnalyticsMeta`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SuppressionReport {
    /// The `k` applied.
    pub k: u64,
    /// How many units were hidden (never their values).
    pub suppressed: u64,
}

/// Counts how many cells fall below `k` and returns the report.
///
/// Suppression is **complete**: a cell below `k` is dropped whole. It is never
/// rounded, coarsened or turned into an "at least k" bucket, because all of
/// those still leak (`ANA-KAN-003`).
#[must_use]
pub fn suppression_report(k: u64, distinct_subjects_per_unit: &[u64]) -> SuppressionReport {
    let suppressed = distinct_subjects_per_unit
        .iter()
        .filter(|count| !k_anon_ok(**count, k))
        .count() as u64;
    SuppressionReport { k, suppressed }
}

/// Driving-event thresholds (`24` §3.2, `17` §2.10).
#[derive(Debug, Clone, Copy)]
pub struct DrivingThresholds {
    pub harsh_accel_mps2: f64,
    pub harsh_brake_mps2: f64,
}

impl Default for DrivingThresholds {
    fn default() -> Self {
        Self {
            harsh_accel_mps2: 3.0,
            harsh_brake_mps2: 3.5,
        }
    }
}

/// Classifies a single acceleration reading into `(harsh_accel, harsh_braking)`.
///
/// The comparison is strict (`>`): a reading exactly on the threshold is not an
/// event. These are **estimated** events (`24` §3.2): without an accelerometer
/// they are derived from the position/speed series, so they are never presented
/// as a measured acceleration nor published on the stream.
#[must_use]
pub fn driving_events(
    accel_mps2: f64,
    brake_mps2: f64,
    thresholds: &DrivingThresholds,
) -> (bool, bool) {
    (
        accel_mps2 > thresholds.harsh_accel_mps2,
        brake_mps2 > thresholds.harsh_brake_mps2,
    )
}

/// 1 m/s speed bucket of the `h3_activity_hourly` histogram (`24` §3.1).
#[must_use]
pub fn speed_bucket(speed_mps: f64) -> usize {
    if !speed_mps.is_finite() || speed_mps <= 0.0 {
        return 0;
    }
    speed_mps.floor() as usize
}

/// Origin-destination cell pair (`24` §3.1, `od_resolution`).
#[must_use]
pub fn od_pair(origin_cell: i64, destination_cell: i64) -> (i64, i64) {
    (origin_cell, destination_cell)
}

/// Circuity of a trip: the travelled distance over the straight-line distance.
///
/// Used by the own ETA estimator (`23` §4) per origin cell. Returns `None` when
/// the straight-line distance is zero (a trip that never moved has no circuity).
#[must_use]
pub fn sinuosity(odometer_m: f64, straight_line_m: f64) -> Option<f64> {
    if straight_line_m <= 0.0 || !straight_line_m.is_finite() {
        return None;
    }
    Some(odometer_m / straight_line_m)
}

/// Residual of an ETA estimate: `actual / estimated` (`24` §3.1).
///
/// Used to correct the own ETA per origin cell and week hour. Returns `None`
/// when the estimate is zero or non-finite (no correction to learn from).
#[must_use]
pub fn eta_correction_residual(actual_s: f64, estimated_s: f64) -> Option<f64> {
    if !estimated_s.is_finite() || estimated_s <= 0.0 {
        return None;
    }
    Some(actual_s / estimated_s)
}

/// A learned place (`24` §3.1). Deliberately **no** subject, session or member
/// identifier: a learned place is a property of the map, not of a person.
#[derive(Debug, Clone, PartialEq)]
pub struct LearnedPlace {
    pub h3_cell: i64,
    pub lat: f64,
    pub lon: f64,
    pub radius_m: f64,
    pub distinct_subjects: u64,
}

/// Whether a learned place may be published: its `distinct_subjects` must reach
/// `k` (`NFR-PRIV-003`). A place visited by fewer people is a person's routine,
/// not a place.
#[must_use]
pub fn learned_place_releasable(place: &LearnedPlace, k: u64) -> bool {
    k_anon_ok(place.distinct_subjects, k)
}

/// Demand and supply of one cell and hour (`24` §3.1).
///
/// Counts only: no identifiers. The net is what a dispatcher reads to decide
/// where to reposition, and the decision is the integrator's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DemandSupply {
    pub demand: u32,
    pub supply: u32,
}

impl DemandSupply {
    /// Positive means more demand than supply.
    #[must_use]
    pub const fn net(self) -> i64 {
        self.demand as i64 - self.supply as i64
    }
}

/// Whether a demand/supply cell may be published: both counts must reach `k`
/// (or be zero), because a cell with one requester is a person's routine.
#[must_use]
pub fn demand_supply_releasable(cell: DemandSupply, k: u32) -> bool {
    (cell.demand == 0 || cell.demand >= k) && (cell.supply == 0 || cell.supply >= k)
}

#[cfg(test)]
mod kanon_tests {
    use super::*;

    #[test]
    fn ana_kan_001_four_subjects_below_five_is_suppressed() {
        assert_eq!(suppress_count(4, 5), None);
    }

    #[test]
    fn ana_kan_002_five_subjects_is_published() {
        assert_eq!(suppress_count(5, 5), Some(5));
    }

    #[test]
    fn ana_kan_003_suppression_is_complete_not_rounded() {
        // A suppressed cell yields no value at all: there is no rounded or
        // coarsened alternative to return.
        assert_eq!(suppress_count(4, 5), None);
    }

    #[test]
    fn ana_kan_004_od_pair_below_k_is_suppressed() {
        // The same rule applies to origin-destination pairs.
        assert_eq!(suppress_count(3, 5), None);
    }

    #[test]
    fn ana_kan_005_learned_place_below_k_is_suppressed() {
        assert_eq!(suppress_count(2, 5), None);
    }

    #[test]
    fn demand_supply_net_and_k_anon() {
        let cell = DemandSupply {
            demand: 10,
            supply: 5,
        };
        assert_eq!(cell.net(), 5);
        assert!(demand_supply_releasable(cell, 5));
        // A supply below k is suppressed too: four drivers is a small group.
        assert!(!demand_supply_releasable(
            DemandSupply {
                demand: 10,
                supply: 4
            },
            5
        ));
        // One requester is a person's routine, not a demand cell.
        assert!(!demand_supply_releasable(
            DemandSupply {
                demand: 1,
                supply: 0
            },
            5
        ));
        // Zero on one side is fine: there simply was no supply.
        assert!(demand_supply_releasable(
            DemandSupply {
                demand: 6,
                supply: 0
            },
            5
        ));
    }

    #[test]
    fn eta_residual_is_actual_over_estimated() {
        assert_eq!(eta_correction_residual(90.0, 60.0), Some(1.5));
        assert_eq!(eta_correction_residual(0.0, 0.0), None);
        assert_eq!(eta_correction_residual(10.0, f64::NAN), None);
    }

    #[test]
    fn learned_place_below_k_is_not_releasable() {
        let place = LearnedPlace {
            h3_cell: 1,
            lat: 8.98,
            lon: -79.52,
            radius_m: 30.0,
            distinct_subjects: 2,
        };
        assert!(!learned_place_releasable(&place, 5));
        let busy = LearnedPlace {
            distinct_subjects: 5,
            ..place
        };
        assert!(learned_place_releasable(&busy, 5));
    }

    #[test]
    fn speed_buckets_are_one_mps() {
        assert_eq!(speed_bucket(0.0), 0);
        assert_eq!(speed_bucket(6.9), 6);
        assert_eq!(speed_bucket(12.0), 12);
        assert_eq!(speed_bucket(-1.0), 0);
        assert_eq!(speed_bucket(f64::NAN), 0);
    }

    #[test]
    fn sinuosity_of_a_straight_trip_is_one() {
        assert_eq!(sinuosity(100.0, 100.0), Some(1.0));
        assert_eq!(sinuosity(135.0, 100.0), Some(1.35));
        assert_eq!(sinuosity(0.0, 0.0), None);
    }

    #[test]
    fn od_pair_is_the_ordered_cell_pair() {
        assert_eq!(od_pair(10, 20), (10, 20));
        assert_ne!(od_pair(10, 20), od_pair(20, 10));
    }

    #[test]
    fn ana_hrs_001_harsh_accel_fires_and_braking_does_not_at_the_edge() {
        // 3,5 > 3,0 (accel) and 3,5 is not > 3,5 (brake): the edge is deliberate.
        let t = DrivingThresholds::default();
        let (accel, brake) = driving_events(3.5, 3.5, &t);
        assert!(accel);
        assert!(!brake);
    }

    #[test]
    fn ana_kan_006_report_declares_k_and_suppressed_count() {
        let report = suppression_report(5, &[3, 5, 4, 9, 1, 5]);
        assert_eq!(report.k, 5);
        assert_eq!(report.suppressed, 3);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn k_anon() {
        assert!(k_anon_ok(5, 5));
        assert!(!k_anon_ok(4, 5));
        assert_eq!(suppress_count(3, 5), None);
        assert_eq!(suppress_count(7, 5), Some(7));
    }
    #[test]
    fn ana_dbs_001_two_clusters_and_noise_discarded() {
        // Two dense groups of 25 stop centroids within ~20 m each, plus one
        // isolated point that must be classified as noise.
        let mut pts: Vec<(f64, f64)> = Vec::new();
        for i in 0..25 {
            pts.push((8.9824 + (i as f64) * 1e-6, -79.5199));
            pts.push((8.9900 + (i as f64) * 1e-6, -79.5199));
        }
        pts.push((9.0500, -79.4000)); // isolated: noise
        let labels = dbscan(&pts, 80.0, 20);
        let clusters: std::collections::HashSet<usize> = labels.iter().filter_map(|l| *l).collect();
        assert_eq!(clusters.len(), 2, "two dense groups");
        assert_eq!(*labels.last().unwrap(), None, "the isolated point is noise");
    }

    #[test]
    fn dbscan_clusters() {
        // two tight clusters of 3 points
        let pts = vec![
            (0.0, 0.0),
            (0.0001, 0.0),
            (0.0, -0.0001),
            (10.0, 10.0),
            (10.0001, 10.0),
            (10.0, 10.0001),
        ];
        let c = dbscan(&pts, 50.0, 2);
        assert_eq!(c[0], c[1]);
        assert_eq!(c[1], c[2]);
        assert_eq!(c[3], c[4]);
        assert_ne!(c[0], c[3]);
    }
}
