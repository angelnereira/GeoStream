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

/// Minimum-cost assignment with **worker capacities** (min-cost flow).
///
/// `costs[w][t]` is the cost of assigning task `t` to worker `w`;
/// `capacities[w]` is how many tasks worker `w` may take. Returns the total
/// cost and, for each task, the worker it was assigned to (`None` if left
/// unassigned).
///
/// This is the model the contract asks for: a single courier cannot carry four
/// orders at once, and "nearest worker per task" is not globally optimal. It is
/// solved with successive shortest paths (SPFA) on the classic
/// `source -> workers -> tasks -> sink` network, where each worker->task edge
/// has capacity 1 and each task is connected to the sink with capacity 1.
#[must_use]
pub fn assign_min_cost_flow(costs: &[Vec<f64>], capacities: &[u32]) -> (f64, Vec<Option<usize>>) {
    let workers = costs.len();
    if workers == 0 {
        return (0.0, Vec::new());
    }
    let tasks = costs[0].len();
    if tasks == 0 {
        return (0.0, Vec::new());
    }

    // Node ids: 0 source, 1..=workers workers, workers+1..=workers+tasks tasks,
    // workers+tasks+1 sink.
    let source = 0usize;
    let sink = workers + tasks + 1;
    let n = sink + 1;

    #[derive(Clone, Copy)]
    struct Edge {
        to: usize,
        cap: i64,
        cost: f64,
        rev: usize,
    }
    let mut graph: Vec<Vec<Edge>> = vec![Vec::new(); n];
    fn add_edge(graph: &mut [Vec<Edge>], from: usize, to: usize, cap: i64, cost: f64) {
        let a = graph[from].len();
        let b = graph[to].len();
        graph[from].push(Edge {
            to,
            cap,
            cost,
            rev: b,
        });
        graph[to].push(Edge {
            to: from,
            cap: 0,
            cost: -cost,
            rev: a,
        });
    }

    for (w, cap) in capacities.iter().enumerate() {
        if *cap > 0 {
            add_edge(&mut graph, source, 1 + w, i64::from(*cap), 0.0);
        }
    }
    for (w, row) in costs.iter().enumerate() {
        for (t, cost) in row.iter().enumerate() {
            if cost.is_finite() {
                add_edge(&mut graph, 1 + w, 1 + workers + t, 1, *cost);
            }
        }
    }
    for t in 0..tasks {
        add_edge(&mut graph, 1 + workers + t, sink, 1, 0.0);
    }

    let mut total_cost = 0.0;

    loop {
        // SPFA shortest path by cost.
        let mut dist = vec![f64::INFINITY; n];
        let mut in_queue = vec![false; n];
        let mut prev_node = vec![usize::MAX; n];
        let mut prev_edge = vec![usize::MAX; n];
        dist[source] = 0.0;
        let mut queue = std::collections::VecDeque::new();
        queue.push_back(source);
        in_queue[source] = true;
        while let Some(u) = queue.pop_front() {
            in_queue[u] = false;
            for (ei, e) in graph[u].iter().enumerate() {
                if e.cap > 0 && dist[u] + e.cost < dist[e.to] - 1e-12 {
                    dist[e.to] = dist[u] + e.cost;
                    prev_node[e.to] = u;
                    prev_edge[e.to] = ei;
                    if !in_queue[e.to] {
                        in_queue[e.to] = true;
                        queue.push_back(e.to);
                    }
                }
            }
        }
        if !dist[sink].is_finite() {
            break;
        }
        // Augment by 1 (all task edges have capacity 1).
        let mut node = sink;
        while node != source {
            let u = prev_node[node];
            let ei = prev_edge[node];
            let v = graph[u][ei].to;
            graph[u][ei].cap -= 1;
            let rev = graph[u][ei].rev;
            graph[v][rev].cap += 1;
            let _ = v;
            node = u;
        }
        total_cost += dist[sink];
    }

    // A worker->task edge started with capacity 1; if its residual capacity is
    // now 0, the pair is part of the flow. Reading the residual (instead of
    // recording during augmentation) is what makes cancellations correct.
    let mut mapping = vec![None; tasks];
    for w in 0..workers {
        for e in &graph[1 + w] {
            if (workers + 1..=workers + tasks).contains(&e.to) && e.cap == 0 {
                mapping[e.to - workers - 1] = Some(w);
            }
        }
    }
    (total_cost, mapping)
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
    fn asg_mcf_001_capacity_limits_assignments() {
        // One worker, capacity 3, four tasks: three assigned, the fourth left out.
        let costs = vec![vec![1.0, 2.0, 3.0, 4.0]];
        let (total, mapping) = assign_min_cost_flow(&costs, &[3]);
        let assigned = mapping.iter().filter(|m| m.is_some()).count();
        assert_eq!(assigned, 3);
        assert_eq!(mapping[3], None, "the fourth task must stay unassigned");
        // The three cheapest are chosen.
        assert_eq!(total, 1.0 + 2.0 + 3.0);
    }

    #[test]
    fn asg_mcf_beats_greedy_when_capacities_bind() {
        // Greedy per-task-nearest can strand the global optimum. Worker A is the
        // only one who can take t2 cheaply but is also the best for t1; giving t1
        // to B (slightly more expensive) frees A for t2 and lowers the total.
        let costs = vec![
            vec![1.0, 1.0],   // w0
            vec![2.0, 100.0], // w1
        ];
        let (mcf, _m) = assign_min_cost_flow(&costs, &[1, 1]);
        // Greedy: t0 -> w0 (1.0), t1 -> w1 (100.0) = 101.0
        // Optimal: t0 -> w1 (2.0), t1 -> w0 (1.0) = 3.0
        assert!((mcf - 3.0).abs() < 1e-9, "mcf was {mcf}");
    }

    #[test]
    fn asg_mcf_empty_is_zero() {
        assert_eq!(assign_min_cost_flow(&[], &[]), (0.0, Vec::new()));
        assert_eq!(assign_min_cost_flow(&[vec![]], &[1]), (0.0, Vec::new()));
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
