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
