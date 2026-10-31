//! Grafo vial propio y map-matching HMM (ADR-0032, `23` §7, `24` §3.3).
//!
//! Red sintética alrededor del centro de Ciudad de Panamá (coordenadas
//! ilustrativas, nunca de una persona): una avenida sur de doble sentido y una
//! calle norte de sentido único hacia el este, paralelas, a 80 m, sin conexión.
#![forbid(unsafe_code)]

use gs_geo::{LatLon, destination, haversine_distance};
use gs_mobility::map_matching::{MatchError, MatchParams, TracePoint, match_trace};
use gs_mobility::road_graph::{GraphError, Oneway, OsmNode, OsmWay, RoadClass, RoadGraph};

const ORIGIN: LatLon = LatLon::new(8.98, -79.52);
const SOUTH_WAY: i64 = 100;
const NORTH_WAY: i64 = 200;

fn east(m: f64) -> LatLon {
    destination(ORIGIN, 90.0, m)
}

fn north_of(p: LatLon, m: f64) -> LatLon {
    destination(p, 0.0, m)
}

fn network() -> (Vec<OsmNode>, Vec<OsmWay>) {
    let mut nodes = Vec::new();
    for i in 0..5 {
        let s = east(100.0 * f64::from(i));
        nodes.push(OsmNode {
            id: i64::from(i),
            position: s,
        });
        nodes.push(OsmNode {
            id: 10 + i64::from(i),
            position: north_of(s, 80.0),
        });
    }
    let ways = vec![
        OsmWay {
            id: SOUTH_WAY,
            node_ids: vec![0, 1, 2, 3, 4],
            class: RoadClass::Primary,
            oneway: Oneway::No,
        },
        OsmWay {
            id: NORTH_WAY,
            node_ids: vec![10, 11, 12, 13, 14],
            class: RoadClass::Residential,
            oneway: Oneway::Forward,
        },
    ];
    (nodes, ways)
}

fn graph() -> RoadGraph {
    let (n, w) = network();
    RoadGraph::build(&n, &w).unwrap()
}

/// Parámetros de prueba explícitos; los de producción se calibran (T-E7-06).
const PARAMS: MatchParams = MatchParams {
    sigma_z_m: 5.0,
    beta_m: 5.0,
    candidate_radius_m: 50.0,
    max_candidates: 8,
    max_route_factor: 2.0,
};

fn tp(p: LatLon) -> TracePoint {
    TracePoint {
        position: p,
        accuracy_m: 8.0,
    }
}

#[test]
fn osm_tags_are_classified() {
    assert_eq!(
        RoadClass::from_osm_highway("primary_link"),
        Some(RoadClass::Primary)
    );
    assert_eq!(RoadClass::from_osm_highway("footway"), None);
    assert_eq!(RoadClass::from_osm_highway("cycleway"), None);
    assert_eq!(
        Oneway::from_osm_tags(None, None, RoadClass::Motorway),
        Oneway::Forward
    );
    assert_eq!(
        Oneway::from_osm_tags(None, Some("roundabout"), RoadClass::Tertiary),
        Oneway::Forward
    );
    assert_eq!(
        Oneway::from_osm_tags(Some("-1"), None, RoadClass::Residential),
        Oneway::Backward
    );
    assert_eq!(
        Oneway::from_osm_tags(Some("no"), None, RoadClass::Motorway),
        Oneway::No
    );
}

#[test]
fn build_creates_directed_csr_edges_and_skips_missing_nodes() {
    let (mut nodes, mut ways) = network();
    nodes.push(OsmNode {
        id: 99,
        position: LatLon::new(f64::NAN, 0.0),
    });
    ways.push(OsmWay {
        id: 300,
        node_ids: vec![4, 98, 99],
        class: RoadClass::Service,
        oneway: Oneway::No,
    });
    let g = RoadGraph::build(&nodes, &ways).unwrap();
    // Sur: 4 tramos × 2 sentidos; norte: 4 tramos × 1 sentido.
    assert_eq!(g.edge_count(), 12);
    assert_eq!(g.stats().edges, 12);
    assert_eq!(g.stats().nodes, 10);
    assert_eq!(g.stats().skipped_segments, 2);
    assert!((g.edge_length_m(0) - 100.0).abs() < 0.5);
}

#[test]
fn build_without_edges_is_an_error() {
    assert_eq!(RoadGraph::build(&[], &[]).unwrap_err(), GraphError::Empty);
}

#[test]
fn candidates_are_sorted_and_bounded_by_radius() {
    let g = graph();
    let p = north_of(east(250.0), 10.0);
    let c = g.candidates(p, 50.0, 8);
    assert!(!c.is_empty());
    assert!((c[0].distance_m - 10.0).abs() < 0.5);
    assert_eq!(g.edge_way_id(c[0].edge), SOUTH_WAY);
    assert!(c.windows(2).all(|w| w[0].distance_m <= w[1].distance_m));
    assert!(c.iter().all(|x| x.distance_m <= 50.0));
    assert!(g.candidates(p, 5.0, 8).is_empty());
    assert_eq!(g.candidates(p, 50.0, 1).len(), 1);
}

#[test]
fn route_distance_follows_the_network_and_respects_oneway() {
    let g = graph();
    let a = g.candidates(north_of(east(50.0), 80.0), 5.0, 1)[0];
    let b = g.candidates(north_of(east(250.0), 80.0), 5.0, 1)[0];
    assert_eq!(g.edge_way_id(a.edge), NORTH_WAY);
    let fwd = g.route_distances(&a, &[b], 1_000.0)[0].unwrap();
    assert!((fwd - 200.0).abs() < 1.0, "fwd = {fwd}");
    // Contra el sentido único no hay camino.
    assert_eq!(g.route_distances(&b, &[a], 1_000.0)[0], None);
    // Fuera del límite tampoco.
    assert_eq!(g.route_distances(&a, &[b], 150.0)[0], None);
}

#[test]
fn hmm_keeps_the_trace_on_the_road_it_can_actually_drive() {
    let g = graph();
    // Una muestra ruidosa queda más cerca de la calle norte (35 m) que de la
    // avenida sur (45 m), pero sin conexión entre ambas el camino coherente es
    // seguir en la sur.
    let trace = [
        tp(north_of(east(50.0), 5.0)),
        tp(north_of(east(150.0), 3.0)),
        tp(north_of(east(250.0), 45.0)),
        tp(north_of(east(350.0), 4.0)),
    ];
    let segments = match_trace(&g, &trace, &PARAMS).unwrap();
    assert_eq!(segments.len(), 1);
    let seg = &segments[0];
    assert_eq!(seg.points.len(), 4);
    assert!(
        seg.points
            .iter()
            .all(|p| g.edge_way_id(p.edge) == SOUTH_WAY)
    );
    assert!(
        (seg.matched_distance_m - 300.0).abs() < 2.0,
        "{}",
        seg.matched_distance_m
    );
    let snapped = seg.points[2].position;
    assert!(haversine_distance(snapped, east(250.0)) < 1.0);
}

#[test]
fn driving_against_a_oneway_breaks_the_trace() {
    let g = graph();
    // Hacia el oeste por la calle norte (solo admite este): ninguna transición es posible.
    let trace = [
        tp(north_of(east(350.0), 82.0)),
        tp(north_of(east(250.0), 82.0)),
        tp(north_of(east(150.0), 82.0)),
    ];
    let segments = match_trace(&g, &trace, &PARAMS).unwrap();
    assert_eq!(segments.len(), 3);
    assert!(segments.iter().all(|s| s.points.len() == 1));
}

#[test]
fn a_point_without_candidates_splits_the_trace() {
    let g = graph();
    let trace = [
        tp(east(50.0)),
        tp(east(150.0)),
        tp(north_of(east(150.0), 1_000.0)),
        tp(east(250.0)),
        tp(east(350.0)),
    ];
    let segments = match_trace(&g, &trace, &PARAMS).unwrap();
    assert_eq!(segments.len(), 2);
    assert_eq!(segments[0].points.len(), 2);
    assert_eq!(segments[1].points[0].trace_index, 3);
}

#[test]
fn invalid_parameters_are_rejected() {
    let g = graph();
    let bad = MatchParams {
        beta_m: 0.0,
        ..PARAMS
    };
    assert_eq!(
        match_trace(&g, &[], &bad).unwrap_err(),
        MatchError::InvalidParam("beta_m")
    );
    let bad = MatchParams {
        max_route_factor: 0.5,
        ..PARAMS
    };
    assert!(match_trace(&g, &[], &bad).is_err());
    assert!(match_trace(&g, &[], &PARAMS).unwrap().is_empty());
}
