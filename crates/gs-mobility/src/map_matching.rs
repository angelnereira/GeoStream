//! Map-matching HMM/Viterbi sobre el grafo propio (Newson & Krumm 2009, `docs/24` §3.3).
//!
//! - Emisión gaussiana sobre la distancia a cada candidato, `σ_z = max(sigma_z_m, accuracy)`.
//! - Transición exponencial sobre `|distancia_por_ruta − distancia_ortodrómica|` con `beta_m`.
//! - Dijkstra acotado a `max_route_factor × ortodrómica + 200 m`.
//! - Sin candidatos o sin transición posible, la traza se corta en segmentos.
//!
//! `sigma_z_m` y `beta_m` no tienen valor por defecto aquí: `24` §3.3 exige
//! tomarlos del artículo y calibrarlos con trazas propias (T-E7-06).

use gs_geo::{LatLon, haversine_distance};

use crate::road_graph::{EdgeCandidate, RoadGraph};

/// Holgura fija de la ruta acotada (`24` §3.3: «+ 200 m»).
const ROUTE_SLACK_M: f64 = 200.0;

/// Parámetros del modelo.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MatchParams {
    /// `mm_sigma_z_m`: desviación típica mínima del error GNSS, en metros.
    pub sigma_z_m: f64,
    /// `mm_beta_m`: escala de la transición, en metros.
    pub beta_m: f64,
    /// `mm_candidate_radius_m` (inicial 50).
    pub candidate_radius_m: f64,
    /// `mm_max_candidates` (inicial 8).
    pub max_candidates: usize,
    /// `mm_max_route_factor`.
    pub max_route_factor: f64,
}

/// Error de parámetros o de entrada.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum MatchError {
    /// Algún parámetro no es finito o no es positivo.
    #[error("invalid map-matching parameter: {0}")]
    InvalidParam(&'static str),
}

impl MatchParams {
    fn validate(&self) -> Result<(), MatchError> {
        let positive = |v: f64| v.is_finite() && v > 0.0;
        if !positive(self.sigma_z_m) {
            return Err(MatchError::InvalidParam("sigma_z_m"));
        }
        if !positive(self.beta_m) {
            return Err(MatchError::InvalidParam("beta_m"));
        }
        if !positive(self.candidate_radius_m) {
            return Err(MatchError::InvalidParam("candidate_radius_m"));
        }
        if self.max_candidates == 0 {
            return Err(MatchError::InvalidParam("max_candidates"));
        }
        if !(self.max_route_factor.is_finite() && self.max_route_factor >= 1.0) {
            return Err(MatchError::InvalidParam("max_route_factor"));
        }
        Ok(())
    }
}

/// Muestra de entrada.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TracePoint {
    pub position: LatLon,
    /// Precisión horizontal informada por el dispositivo, en metros.
    pub accuracy_m: f64,
}

/// Muestra ajustada a la vía.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MatchedPoint {
    /// Índice de la muestra en la traza de entrada.
    pub trace_index: usize,
    /// Arista elegida.
    pub edge: u32,
    /// Posición sobre la vía.
    pub position: LatLon,
    /// Distancia en metros entre la muestra y la vía.
    pub offset_m: f64,
}

/// Tramo continuo de la traza ajustado a la red.
#[derive(Debug, Clone, PartialEq)]
pub struct MatchedSegment {
    pub points: Vec<MatchedPoint>,
    /// Distancia recorrida por la red entre los puntos ajustados.
    pub matched_distance_m: f64,
}

struct Step {
    trace_index: usize,
    candidates: Vec<EdgeCandidate>,
    score: Vec<f64>,
    /// Por candidato: (candidato anterior, distancia por ruta desde él).
    back: Vec<Option<(usize, f64)>>,
}

fn emission(c: &EdgeCandidate, sigma: f64) -> f64 {
    let z = c.distance_m / sigma;
    -0.5 * z * z
}

fn finish(steps: &mut Vec<Step>, out: &mut Vec<MatchedSegment>) {
    let Some(last) = steps.last() else { return };
    let mut best = 0;
    for (i, s) in last.score.iter().enumerate() {
        if *s > last.score[best] {
            best = i;
        }
    }
    let mut points = Vec::with_capacity(steps.len());
    let mut distance = 0.0;
    let mut idx = best;
    for step in steps.iter().rev() {
        let c = step.candidates[idx];
        points.push(MatchedPoint {
            trace_index: step.trace_index,
            edge: c.edge,
            position: c.position,
            offset_m: c.distance_m,
        });
        match step.back[idx] {
            Some((prev, d)) => {
                distance += d;
                idx = prev;
            }
            None => break,
        }
    }
    points.reverse();
    out.push(MatchedSegment {
        points,
        matched_distance_m: distance,
    });
    steps.clear();
}

/// Ajusta una traza completa a la red (modo por lotes, Viterbi completo).
///
/// # Errors
/// [`MatchError::InvalidParam`] si algún parámetro es inválido.
pub fn match_trace(
    graph: &RoadGraph,
    trace: &[TracePoint],
    params: &MatchParams,
) -> Result<Vec<MatchedSegment>, MatchError> {
    params.validate()?;
    let mut out = Vec::new();
    let mut steps: Vec<Step> = Vec::new();
    let mut prev_point: Option<LatLon> = None;

    for (i, tp) in trace.iter().enumerate() {
        let candidates = graph.candidates(
            tp.position,
            params.candidate_radius_m,
            params.max_candidates,
        );
        if candidates.is_empty() {
            finish(&mut steps, &mut out);
            prev_point = None;
            continue;
        }
        let sigma = if tp.accuracy_m.is_finite() {
            params.sigma_z_m.max(tp.accuracy_m)
        } else {
            params.sigma_z_m
        };
        let emissions: Vec<f64> = candidates.iter().map(|c| emission(c, sigma)).collect();

        let mut score = vec![f64::NEG_INFINITY; candidates.len()];
        let mut back = vec![None; candidates.len()];
        let mut reachable = false;
        if let (Some(prev), Some(last)) = (prev_point, steps.last()) {
            let gc = haversine_distance(prev, tp.position);
            let limit = params.max_route_factor * gc + ROUTE_SLACK_M;
            for (pi, pc) in last.candidates.iter().enumerate() {
                if last.score[pi] == f64::NEG_INFINITY {
                    continue;
                }
                let routes = graph.route_distances(pc, &candidates, limit);
                for (ci, route) in routes.into_iter().enumerate() {
                    let Some(r) = route else { continue };
                    let s = last.score[pi] - (r - gc).abs() / params.beta_m + emissions[ci];
                    if s > score[ci] {
                        score[ci] = s;
                        back[ci] = Some((pi, r));
                        reachable = true;
                    }
                }
            }
        }
        if !reachable {
            // Primera muestra o transición imposible: empieza un segmento nuevo.
            finish(&mut steps, &mut out);
            score = emissions;
            back = vec![None; candidates.len()];
        }
        steps.push(Step {
            trace_index: i,
            candidates,
            score,
            back,
        });
        prev_point = Some(tp.position);
    }
    finish(&mut steps, &mut out);
    Ok(out)
}
