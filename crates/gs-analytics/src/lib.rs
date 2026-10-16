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
