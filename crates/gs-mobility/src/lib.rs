#![forbid(unsafe_code)]

use gs_geo::{LatLon, geodesic_distance};

#[derive(Debug, Clone, Copy)]
pub struct EtaInput {
    pub origin_lat: f64,
    pub origin_lon: f64,
    pub dest_lat: f64,
    pub dest_lon: f64,
    pub mode_speed_mps: f64,
}

pub fn estimate_eta(i: EtaInput) -> f64 {
    // seconds = distance / speed. Uses ellipsoidal distance.
    let d = geodesic_distance(
        LatLon::new(i.origin_lat, i.origin_lon),
        LatLon::new(i.dest_lat, i.dest_lon),
    )
    .meters;
    if i.mode_speed_mps <= 0.0 {
        return f64::INFINITY;
    }
    d / i.mode_speed_mps
}

/// Hungarian-style optimal assignment (min-cost) for small n. O(n^3).
/// cost matrix n x m (n <= m). Returns (total_cost, mapping i -> j).
pub fn assign(costs: &[Vec<f64>], capacity: usize) -> (f64, Vec<Option<usize>>) {
    let n = costs.len();
    if n == 0 {
        return (0.0, Vec::new());
    }
    let m = costs[0].len();
    // augmented square matrix
    let size = n.max(m);
    let mut a = vec![vec![0.0f64; size]; size];
    for (i, row) in a.iter_mut().enumerate().take(n) {
        for (j, cell) in row.iter_mut().enumerate().take(m) {
            *cell = costs[i][j];
        }
    }
    // large value for dummy edges
    let big = 1e12;
    for row in a.iter_mut().take(n) {
        for cell in row.iter_mut().skip(m) {
            *cell = big;
        }
    }
    for row in a.iter_mut().skip(n) {
        for cell in row.iter_mut() {
            *cell = 0.0;
        }
    }

    // Hungarian algorithm (assignment on square matrix).
    // Standard O(n^3) implementation.
    #[allow(non_snake_case)]
    let mut u = vec![0.0; size + 1];
    #[allow(non_snake_case)]
    let mut v = vec![0.0; size + 1];
    #[allow(non_snake_case)]
    let mut p = vec![0usize; size + 1];
    #[allow(non_snake_case)]
    let way = vec![0usize; size + 1];
    for i in 1..=size {
        p[0] = i;
        let mut j0 = 0usize;
        let mut minv = vec![f64::INFINITY; size + 1];
        let mut used = vec![false; size + 1];
        loop {
            used[j0] = true;
            let i0 = p[j0];
            let mut delta = f64::INFINITY;
            let mut j1 = 0usize;
            for j in 1..=size {
                if used[j] {
                    continue;
                }
                let cur = a[i0 - 1][j - 1] - u[i0] - v[j];
                if cur < minv[j] {
                    minv[j] = cur;
                }
                if minv[j] < delta {
                    delta = minv[j];
                    j1 = j;
                }
            }
            for j in 0..=size {
                if used[j] {
                    u[p[j]] += delta;
                    v[j] -= delta;
                } else {
                    minv[j] -= delta;
                }
            }
            j0 = j1;
            if p[j0] == 0 {
                break;
            }
        }
        loop {
            let j1 = way[j0];
            p[j0] = p[j1];
            j0 = j1;
            if j0 == 0 {
                break;
            }
        }
    }
    // p[j] = row assigned to column j
    let mut col_to_row = vec![0usize; size];
    for j in 1..=size {
        if p[j] != 0 {
            col_to_row[p[j] - 1] = j - 1;
        }
    }
    // build mapping i->j for i in 0..n
    let mut mapping = vec![None; n];
    let mut total = 0.0;
    for i in 0..n {
        let j = col_to_row[i];
        if j < m {
            mapping[i] = Some(j);
            total += costs[i][j];
        }
    }
    // capacity ignored for now (single assignment per task)
    let _ = capacity;
    (total, mapping)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn eta_basic() {
        let e = estimate_eta(EtaInput {
            origin_lat: 8.98,
            origin_lon: -79.52,
            dest_lat: 8.99,
            dest_lon: -79.52,
            mode_speed_mps: 10.0,
        });
        assert!(e.is_finite() && e > 0.0);
    }
    #[test]
    fn assign_simple() {
        let costs = vec![vec![1.0, 2.0], vec![2.0, 1.0]];
        let (t, m) = assign(&costs, 1);
        assert_eq!(t, 2.0);
        assert_eq!(m, vec![Some(0), Some(1)]);
    }
}
