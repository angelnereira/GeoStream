#![forbid(unsafe_code)]

use gs_geo::{LatLon, geodesic_distance};

/// Travel mode with the documented default speed (`docs/17` §2.11).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EtaMode {
    Driving,
    Motorcycle,
    Cycling,
    Walking,
}

impl EtaMode {
    #[must_use]
    pub const fn speed_mps(self) -> f64 {
        match self {
            Self::Driving => 6.9,
            Self::Motorcycle => 8.3,
            Self::Cycling => 4.2,
            Self::Walking => 1.3,
        }
    }
}

/// Default circuity factor (`eta_default_circuity`): road distance is ~35 %
/// longer than the straight line.
pub const ETA_DEFAULT_CIRCUITY: f64 = 1.35;

/// Own ETA estimate: `straight_line * circuity / mode_speed`.
///
/// Returns `None` when the source is not known (`LEARNED` without a learned
/// route): `null` means "I don't know" and must never render as `0`.
#[must_use]
pub fn estimate_eta(
    origin: (f64, f64),
    dest: (f64, f64),
    mode: EtaMode,
    circuity: f64,
    source_known: bool,
) -> Option<f64> {
    if !source_known {
        return None;
    }
    let d = geodesic_distance(LatLon::new(origin.0, origin.1), LatLon::new(dest.0, dest.1)).meters;
    let speed = mode.speed_mps();
    if speed <= 0.0 {
        return None;
    }
    Some(d * circuity / speed)
}

/// Whether a `rows x cols` matrix fits within `eta_max_matrix_cells`.
#[must_use]
pub fn matrix_within_limit(rows: u64, cols: u64, max_cells: u64) -> bool {
    rows.saturating_mul(cols) <= max_cells
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
    fn eta_est_001_driving_matches_vector() {
        // 844,9 m x 1,35 / 6,9 m/s ≈ 165 s.
        let e = estimate_eta(
            (8.9824, -79.5199),
            (8.9900, -79.5199),
            EtaMode::Driving,
            ETA_DEFAULT_CIRCUITY,
            true,
        )
        .unwrap();
        assert!((164.0..=166.0).contains(&e), "eta was {e}");
    }

    #[test]
    fn eta_est_002_walking_is_much_longer() {
        let e = estimate_eta(
            (8.9824, -79.5199),
            (8.9900, -79.5199),
            EtaMode::Walking,
            ETA_DEFAULT_CIRCUITY,
            true,
        )
        .unwrap();
        assert!((870.0..=880.0).contains(&e), "eta was {e}");
    }

    #[test]
    fn eta_est_005_unknown_source_is_none_not_zero() {
        assert_eq!(
            estimate_eta(
                (8.9824, -79.5199),
                (-33.4489, -70.6693),
                EtaMode::Driving,
                ETA_DEFAULT_CIRCUITY,
                false
            ),
            None
        );
    }

    #[test]
    fn eta_est_003_004_matrix_limit() {
        assert!(matrix_within_limit(25, 25, 625));
        assert!(!matrix_within_limit(30, 30, 625));
    }
    #[test]
    fn assign_simple() {
        let costs = vec![vec![1.0, 2.0], vec![2.0, 1.0]];
        let (t, m) = assign(&costs, 1);
        assert_eq!(t, 2.0);
        assert_eq!(m, vec![Some(0), Some(1)]);
    }

    /// Exhaustive minimum over all permutations (the oracle the vectors demand).
    fn exhaustive_min(costs: &[Vec<f64>]) -> f64 {
        let n = costs.len();
        let mut idx: Vec<usize> = (0..costs[0].len()).collect();
        let mut best = f64::INFINITY;
        // Heap's algorithm over column permutations of size n.
        fn permute(k: usize, idx: &mut Vec<usize>, costs: &[Vec<f64>], best: &mut f64) {
            if k == costs.len() {
                let sum: f64 = (0..costs.len()).map(|i| costs[i][idx[i]]).sum();
                if sum < *best {
                    *best = sum;
                }
                return;
            }
            for i in k..idx.len() {
                idx.swap(k, i);
                permute(k + 1, idx, costs, best);
                idx.swap(k, i);
            }
        }
        if idx.len() >= n {
            permute(0, &mut idx, costs, &mut best);
        }
        best
    }

    #[test]
    fn asg_hun_is_globally_optimal_not_greedy() {
        let costs = vec![
            vec![1.0, 4.0, 3.0],
            vec![2.0, 1.0, 5.0],
            vec![3.0, 2.0, 2.0],
        ];
        let (total, _m) = assign(&costs, 1);
        let oracle = exhaustive_min(&costs);
        assert!(
            (total - oracle).abs() < 1e-9,
            "hungarian {total} != exhaustive {oracle}"
        );
    }

    #[test]
    fn asg_hun_distance_matrix_matches_oracle() {
        // The ASG-HUN-002 arrangement, compared against exhaustive search.
        let lat = |d: f64| d * 111_195.0;
        let costs = vec![
            vec![lat(0.0001), lat(0.0076)],
            vec![lat(0.0005), lat(0.0070)],
        ];
        let (total, _m) = assign(&costs, 1);
        assert!((total - exhaustive_min(&costs)).abs() < 1e-9);
    }
}
