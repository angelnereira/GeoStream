//! Grafo vial propio construido con datos de OpenStreetMap (ADR-0032, `docs/23` §7).
//!
//! Puro y sin I/O: recibe nodos y vías ya leídos (el lector de `.osm.pbf` es un
//! adaptador aparte) y produce un grafo dirigido en formato CSR con un índice
//! espacial de aristas en rejilla. Los datos OSM están bajo ODbL: quien muestre
//! resultados derivados debe atribuir «© OpenStreetMap contributors» (`13` §2).

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

use gs_geo::{LatLon, LocalFrame, haversine_distance};

/// Metros por grado de latitud en la esfera media (para dimensionar la rejilla).
const M_PER_DEG_LAT: f64 = 111_195.0;

/// Lado de la celda del índice espacial, en grados (~110 m de latitud).
const GRID_CELL_DEG: f64 = 0.001;

/// Clase de vía, a partir de la etiqueta `highway` de OSM. Perfil vehicular.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RoadClass {
    Motorway,
    Trunk,
    Primary,
    Secondary,
    Tertiary,
    Unclassified,
    Residential,
    LivingStreet,
    Service,
}

impl RoadClass {
    /// Clase para un valor de `highway`; `None` para vías no transitables en
    /// vehículo (`footway`, `path`, `cycleway`, `steps`…) o desconocidas.
    #[must_use]
    pub fn from_osm_highway(value: &str) -> Option<Self> {
        Some(match value {
            "motorway" | "motorway_link" => Self::Motorway,
            "trunk" | "trunk_link" => Self::Trunk,
            "primary" | "primary_link" => Self::Primary,
            "secondary" | "secondary_link" => Self::Secondary,
            "tertiary" | "tertiary_link" => Self::Tertiary,
            "unclassified" | "road" => Self::Unclassified,
            "residential" => Self::Residential,
            "living_street" => Self::LivingStreet,
            "service" => Self::Service,
            _ => return None,
        })
    }
}

/// Sentido de circulación de una vía.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Oneway {
    /// Doble sentido.
    No,
    /// Solo en el orden de los nodos de la vía.
    Forward,
    /// Solo en el orden inverso.
    Backward,
}

impl Oneway {
    /// Interpreta las etiquetas `oneway` y `junction` de OSM. Las autopistas y
    /// las rotondas son de un solo sentido salvo que se indique lo contrario.
    #[must_use]
    pub fn from_osm_tags(oneway: Option<&str>, junction: Option<&str>, class: RoadClass) -> Self {
        match oneway {
            Some("yes" | "true" | "1") => Self::Forward,
            Some("-1" | "reverse") => Self::Backward,
            Some("no" | "false" | "0") => Self::No,
            _ if class == RoadClass::Motorway => Self::Forward,
            _ if matches!(junction, Some("roundabout" | "circular")) => Self::Forward,
            _ => Self::No,
        }
    }
}

/// Nodo OSM.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OsmNode {
    pub id: i64,
    pub position: LatLon,
}

/// Vía OSM ya clasificada.
#[derive(Debug, Clone, PartialEq)]
pub struct OsmWay {
    pub id: i64,
    pub node_ids: Vec<i64>,
    pub class: RoadClass,
    pub oneway: Oneway,
}

/// Error de construcción del grafo.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GraphError {
    /// Ninguna vía produjo aristas.
    #[error("the road graph has no edges")]
    Empty,
    /// Más nodos o aristas de los que caben en índices de 32 bits.
    #[error("the road graph exceeds 2^32 nodes or edges")]
    TooLarge,
}

/// Contadores de la construcción, para el informe de `geostream graph build`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BuildStats {
    pub nodes: usize,
    pub edges: usize,
    /// Tramos descartados porque un nodo no estaba en el extracto (vías
    /// recortadas en el borde de la región) o tenía coordenadas inválidas.
    pub skipped_segments: usize,
}

/// Posición de un punto proyectado sobre una arista.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EdgeCandidate {
    /// Índice de la arista.
    pub edge: u32,
    /// Fracción recorrida desde la cola hasta la cabeza, en `[0, 1]`.
    pub fraction: f64,
    /// Distancia en metros del punto original a la arista.
    pub distance_m: f64,
    /// Punto proyectado sobre la arista.
    pub position: LatLon,
}

/// Grafo dirigido en CSR con índice espacial de aristas.
#[derive(Debug, Clone)]
pub struct RoadGraph {
    positions: Vec<LatLon>,
    first_out: Vec<u32>,
    head: Vec<u32>,
    tail: Vec<u32>,
    length_m: Vec<f64>,
    class: Vec<RoadClass>,
    way_id: Vec<i64>,
    grid: HashMap<(i32, i32), Vec<u32>>,
    stats: BuildStats,
}

fn cell_of(p: LatLon) -> (i32, i32) {
    (
        (p.lat / GRID_CELL_DEG).floor() as i32,
        (p.lon / GRID_CELL_DEG).floor() as i32,
    )
}

impl RoadGraph {
    /// Construye el grafo con los nodos referenciados por `ways`.
    ///
    /// # Errors
    /// [`GraphError::Empty`] si no queda ninguna arista; [`GraphError::TooLarge`]
    /// si no cabe en índices de 32 bits.
    pub fn build(nodes: &[OsmNode], ways: &[OsmWay]) -> Result<Self, GraphError> {
        let by_id: HashMap<i64, LatLon> = nodes
            .iter()
            .filter(|n| n.position.is_valid())
            .map(|n| (n.id, n.position))
            .collect();
        let mut index: HashMap<i64, u32> = HashMap::new();
        let mut positions: Vec<LatLon> = Vec::new();
        let mut intern = |id: i64, pos: LatLon| -> Result<u32, GraphError> {
            if let Some(&i) = index.get(&id) {
                return Ok(i);
            }
            let i = u32::try_from(positions.len()).map_err(|_| GraphError::TooLarge)?;
            positions.push(pos);
            index.insert(id, i);
            Ok(i)
        };

        // (cola, cabeza, longitud, clase, vía)
        let mut edges: Vec<(u32, u32, f64, RoadClass, i64)> = Vec::new();
        let mut skipped = 0usize;
        for way in ways {
            for pair in way.node_ids.windows(2) {
                let (Some(&pa), Some(&pb)) = (by_id.get(&pair[0]), by_id.get(&pair[1])) else {
                    skipped += 1;
                    continue;
                };
                if pair[0] == pair[1] {
                    continue;
                }
                let a = intern(pair[0], pa)?;
                let b = intern(pair[1], pb)?;
                let len = haversine_distance(pa, pb);
                if way.oneway != Oneway::Backward {
                    edges.push((a, b, len, way.class, way.id));
                }
                if way.oneway != Oneway::Forward {
                    edges.push((b, a, len, way.class, way.id));
                }
            }
        }
        if edges.is_empty() {
            return Err(GraphError::Empty);
        }
        u32::try_from(edges.len()).map_err(|_| GraphError::TooLarge)?;
        edges.sort_by_key(|e| e.0);

        let n = positions.len();
        let mut first_out = vec![0u32; n + 1];
        for e in &edges {
            first_out[e.0 as usize + 1] += 1;
        }
        for i in 0..n {
            first_out[i + 1] += first_out[i];
        }

        let mut graph = Self {
            positions,
            first_out,
            head: edges.iter().map(|e| e.1).collect(),
            tail: edges.iter().map(|e| e.0).collect(),
            length_m: edges.iter().map(|e| e.2).collect(),
            class: edges.iter().map(|e| e.3).collect(),
            way_id: edges.iter().map(|e| e.4).collect(),
            grid: HashMap::new(),
            stats: BuildStats {
                nodes: n,
                edges: edges.len(),
                skipped_segments: skipped,
            },
        };
        graph.index_edges();
        Ok(graph)
    }

    fn index_edges(&mut self) {
        for e in 0..self.head.len() {
            let a = cell_of(self.positions[self.tail[e] as usize]);
            let b = cell_of(self.positions[self.head[e] as usize]);
            for la in a.0.min(b.0)..=a.0.max(b.0) {
                for lo in a.1.min(b.1)..=a.1.max(b.1) {
                    self.grid.entry((la, lo)).or_default().push(e as u32);
                }
            }
        }
    }

    /// Contadores de construcción.
    #[must_use]
    pub const fn stats(&self) -> BuildStats {
        self.stats
    }

    /// Número de aristas dirigidas.
    #[must_use]
    pub fn edge_count(&self) -> usize {
        self.head.len()
    }

    /// Longitud de la arista en metros.
    #[must_use]
    pub fn edge_length_m(&self, edge: u32) -> f64 {
        self.length_m[edge as usize]
    }

    /// Clase de vía de la arista.
    #[must_use]
    pub fn edge_class(&self, edge: u32) -> RoadClass {
        self.class[edge as usize]
    }

    /// ID de la vía OSM de la que sale la arista.
    #[must_use]
    pub fn edge_way_id(&self, edge: u32) -> i64 {
        self.way_id[edge as usize]
    }

    /// Extremos `(cola, cabeza)` de la arista.
    #[must_use]
    pub fn edge_endpoints(&self, edge: u32) -> (LatLon, LatLon) {
        (
            self.positions[self.tail[edge as usize] as usize],
            self.positions[self.head[edge as usize] as usize],
        )
    }

    fn project(&self, edge: u32, p: LatLon) -> EdgeCandidate {
        let (a, b) = self.edge_endpoints(edge);
        let frame = LocalFrame::new(p);
        let (ax, ay) = frame.to_enu(a);
        let (bx, by) = frame.to_enu(b);
        let (dx, dy) = (bx - ax, by - ay);
        let len2 = dx * dx + dy * dy;
        let t = if len2 == 0.0 {
            0.0
        } else {
            ((-ax * dx - ay * dy) / len2).clamp(0.0, 1.0)
        };
        let (px, py) = (ax + t * dx, ay + t * dy);
        EdgeCandidate {
            edge,
            fraction: t,
            distance_m: (px * px + py * py).sqrt(),
            position: frame.from_enu(px, py),
        }
    }

    /// Aristas a menos de `radius_m` de `p`, ordenadas por distancia, como
    /// máximo `max` (`mm_candidate_radius_m`, `mm_max_candidates`, `24` §3.3).
    #[must_use]
    pub fn candidates(&self, p: LatLon, radius_m: f64, max: usize) -> Vec<EdgeCandidate> {
        if !p.is_valid() || !radius_m.is_finite() || radius_m <= 0.0 || max == 0 {
            return Vec::new();
        }
        let dlat = radius_m / M_PER_DEG_LAT;
        let dlon = radius_m / (M_PER_DEG_LAT * p.lat.to_radians().cos().abs().max(0.01));
        let lo = cell_of(LatLon::new(p.lat - dlat, p.lon - dlon));
        let hi = cell_of(LatLon::new(p.lat + dlat, p.lon + dlon));
        let mut edges: Vec<u32> = Vec::new();
        for la in lo.0..=hi.0 {
            for ln in lo.1..=hi.1 {
                if let Some(v) = self.grid.get(&(la, ln)) {
                    edges.extend_from_slice(v);
                }
            }
        }
        edges.sort_unstable();
        edges.dedup();
        let mut out: Vec<EdgeCandidate> = edges
            .into_iter()
            .map(|e| self.project(e, p))
            .filter(|c| c.distance_m <= radius_m)
            .collect();
        out.sort_by(|a, b| a.distance_m.total_cmp(&b.distance_m));
        out.truncate(max);
        out
    }

    /// Distancia por la red desde `from` hasta cada destino de `to`, respetando
    /// el sentido de las aristas. `None` si supera `limit_m` o no hay camino.
    ///
    /// Un solo Dijkstra acotado por origen (uno a muchos).
    #[must_use]
    pub fn route_distances(
        &self,
        from: &EdgeCandidate,
        to: &[EdgeCandidate],
        limit_m: f64,
    ) -> Vec<Option<f64>> {
        let from_len = self.edge_length_m(from.edge);
        let start_cost = (1.0 - from.fraction) * from_len;
        let start = self.head[from.edge as usize];

        let mut dist: HashMap<u32, f64> = HashMap::new();
        let mut heap = BinaryHeap::new();
        if start_cost <= limit_m {
            dist.insert(start, start_cost);
            heap.push(State {
                cost: start_cost,
                node: start,
            });
        }
        while let Some(State { cost, node }) = heap.pop() {
            if cost > dist.get(&node).copied().unwrap_or(f64::INFINITY) {
                continue;
            }
            let (lo, hi) = (
                self.first_out[node as usize] as usize,
                self.first_out[node as usize + 1] as usize,
            );
            for e in lo..hi {
                let next = cost + self.length_m[e];
                if next > limit_m {
                    continue;
                }
                let h = self.head[e];
                if next < dist.get(&h).copied().unwrap_or(f64::INFINITY) {
                    dist.insert(h, next);
                    heap.push(State {
                        cost: next,
                        node: h,
                    });
                }
            }
        }

        to.iter()
            .map(|t| {
                if t.edge == from.edge && t.fraction >= from.fraction {
                    return Some((t.fraction - from.fraction) * from_len);
                }
                let tail = self.tail[t.edge as usize];
                let d = dist.get(&tail)? + t.fraction * self.edge_length_m(t.edge);
                (d <= limit_m).then_some(d)
            })
            .collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct State {
    cost: f64,
    node: u32,
}

impl Eq for State {}

impl Ord for State {
    fn cmp(&self, other: &Self) -> Ordering {
        // Montículo de mínimos: invierte el orden del coste.
        other
            .cost
            .total_cmp(&self.cost)
            .then_with(|| self.node.cmp(&other.node))
    }
}

impl PartialOrd for State {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
