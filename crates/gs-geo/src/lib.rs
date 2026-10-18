//! Pure geodesy for GeoStream.
//!
//! Implements the spherical and ellipsoidal geometry that the tracking engine
//! (`22`), the mobility tools (`23`), the history simplifier (`24`) and the
//! client core (`26`) all need, so that the formula exists once.
//!
//! # What this crate is for
//!
//! Three consumers with different needs:
//!
//! - **`gs-track` (server, hot path).** O(1) state per entity and **zero
//!   allocations per sample** (`22` P-E2). [`LocalFrame`] is the tangent
//!   ENU frame the Kalman filter works in; it is `Copy` and allocation-free.
//! - **`gs-client-core` (device).** Must compile to
//!   `wasm32-unknown-unknown` (`26` §1.2 rule 4), so no `tokio`, no I/O and no
//!   platform APIs. Everything here is `no_std`-shaped arithmetic.
//! - **`gs-mobility` / `gs-analytics`.** Distances, bearings and polyline
//!   simplification, often over thousands of points at once.
//!
//! # Two kinds of distance, and why both exist
//!
//! [`haversine_distance`] treats the Earth as a sphere; [`geodesic_distance`]
//! treats it as the WGS84 ellipsoid. They differ by up to ~0.5 % depending on
//! latitude and direction. [`geodesic_distance`] does **not** silently pick one:
//! it returns a [`GeodesicDistance`] carrying the [`DistanceMethod`] actually
//! used, because a caller that needs accuracy (map-matching, speed limits)
//! must be able to tell that it did not get it.
//!
//! # On precision
//!
//! The ellipsoidal formulas are Vincenty's, as implemented by the `geo` crate in
//! the pinned version. Vincenty does **not** converge for near-antipodal
//! points; there, [`geodesic_distance`] falls back to the spherical value and
//! reports [`DistanceMethod::HaversineFallback`] rather than returning a wrong
//! ellipsoidal number. In the product this never happens for trackable pairs,
//! which are metres apart, but a public API should not pretend otherwise.

#![forbid(unsafe_code)]
#![allow(clippy::excessive_precision)] // WGS84 constants are exact decimals.

use geo::algorithm::VincentyDistance;
use geo::algorithm::line_measures::{Bearing, Distance};
use geo::{HaversineMeasure, Point};

/// Mean radius of the Earth (IUGG / Moritz 2000), in metres.
///
/// This is the radius the haversine formula is defined against. It is also the
/// value `geo::Haversine` uses, so [`haversine_distance`] and `geo`'s own
/// haversine agree to the last bit.
pub const EARTH_MEAN_RADIUS_M: f64 = 6_371_008.8;

/// WGS84 semi-major axis, in metres.
///
/// Declares the ellipsoid [`geodesic_distance`] works on. It is not passed to
/// the formula: `geo` carries WGS84 internally, and duplicating the parameter
/// would let the two drift apart silently. These two constants exist so the
/// model is **stated** rather than implied, and
/// `one_degree_of_latitude_is_the_meridian_arc` / `constants_match_their_documented_sources`
/// check that what `geo` actually implements is this ellipsoid.
pub const EARTH_SEMIMAJOR_AXIS_M: f64 = 6_378_137.0;

/// WGS84 flattening: `(a - b) / a`.
pub const EARTH_FLATTENING: f64 = 1.0 / 298.257_223_563;

/// Spherical model used by the haversine helpers.
const SPHERE: HaversineMeasure = HaversineMeasure::new(EARTH_MEAN_RADIUS_M);

/// A geographic coordinate in WGS 84, degrees.
///
/// `Copy` and allocation-free: this type crosses the hot path of the tracking
/// engine on every sample (`22` P-E2).
///
/// It is **not** validated on construction. Validation is
/// `gs_domain::location::Position`'s job (`09` §3.1, rules 4–5) and belongs at
/// the boundary, not here: this crate is also used on already-validated internal
/// values where re-validating would be waste. Use [`LatLon::is_valid`] when the
/// input has not been through the boundary.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LatLon {
    /// Degrees north of the equator, `[-90, 90]`.
    pub lat: f64,
    /// Degrees east of Greenwich, `[-180, 180]`.
    pub lon: f64,
}

impl LatLon {
    /// Builds a coordinate without validating it. See [`LatLon`].
    #[must_use]
    pub const fn new(lat: f64, lon: f64) -> Self {
        Self { lat, lon }
    }

    /// Whether the coordinate is within the WGS 84 domain and finite.
    #[must_use]
    pub fn is_valid(self) -> bool {
        self.lat.is_finite()
            && self.lon.is_finite()
            && (-90.0..=90.0).contains(&self.lat)
            && (-180.0..=180.0).contains(&self.lon)
    }
}

/// How a distance was computed.
///
/// Returned instead of a bare `f64` so that a caller which needs ellipsoidal
/// accuracy can detect when it did not get it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DistanceMethod {
    /// Haversine on the IUGG mean sphere.
    Haversine,
    /// Vincenty on the WGS84 ellipsoid: converged.
    Vincenty,
    /// Vincenty did not converge (near-antipodal pair) and the spherical value
    /// was used instead. The number is usable for short ranges and **wrong** for
    /// long ones; the flag exists so nobody treats it as ellipsoidal.
    HaversineFallback,
}

/// An ellipsoidal distance plus the method that produced it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GeodesicDistance {
    /// Metres.
    pub meters: f64,
    /// Which formula actually produced `meters`.
    pub method: DistanceMethod,
}

/// Great-circle distance in metres (spherical model).
#[must_use]
pub fn haversine_distance(a: LatLon, b: LatLon) -> f64 {
    SPHERE.distance(point(a), point(b))
}

/// Ellipsoidal distance in metres (WGS84), reporting the method used.
///
/// Falls back to [`DistanceMethod::HaversineFallback`] when Vincenty fails to
/// converge. See the [module documentation](self) for why.
#[must_use]
pub fn geodesic_distance(a: LatLon, b: LatLon) -> GeodesicDistance {
    match point(a).vincenty_distance(&point(b)) {
        Ok(meters) => GeodesicDistance {
            meters,
            method: DistanceMethod::Vincenty,
        },
        // Vincenty no converge: par casi antípodas. Se usa la esfera y se dice.
        Err(_) => GeodesicDistance {
            meters: haversine_distance(a, b),
            method: DistanceMethod::HaversineFallback,
        },
    }
}

/// Initial bearing from `a` to `b`, in degrees clockwise from north, `[0, 360)`.
///
/// Two coincident points have no bearing; the function returns `0.0` rather than
/// a NaN, because a NaN in this value would propagate into the filter's state and
/// poison every later sample. Callers that must distinguish "no bearing" from
/// "due north" check [`LatLon`] equality themselves.
#[must_use]
pub fn bearing(a: LatLon, b: LatLon) -> f64 {
    // Rumbo inicial sobre la esfera, no azimut elipsoidal. Tres razones, en orden
    // de peso: es el modelo con el que [`haversine_distance`] mide, y un filtro
    // que deriva velocidad de una distancia y una dirección de modelos
    // distintos mezcla dos aproximaciones; es más barato en el camino caliente
    // (`22` P-E2); y la diferencia con el elipsoide es de ~0,1°, muy por debajo
    // de la precisión declarada por cualquier receptor GNSS.
    let value = SPHERE.bearing(point(a), point(b));
    if value.is_finite() {
        value.rem_euclid(360.0)
    } else {
        0.0
    }
}

/// The point reached from `origin` after travelling `distance_m` along `bearing_deg`.
///
/// Uses the spherical destination formula: accurate to a few metres for the
/// ranges the product deals with (metres to tens of kilometres), which is all
/// the sampling and prediction code needs.
#[must_use]
pub fn destination(origin: LatLon, bearing_deg: f64, distance_m: f64) -> LatLon {
    if distance_m == 0.0 || !distance_m.is_finite() {
        return origin;
    }
    let angular = distance_m / EARTH_MEAN_RADIUS_M;
    let (sin_lat, cos_lat) = origin.lat.to_radians().sin_cos();
    let bearing_rad = bearing_deg.to_radians();
    let sin_b = bearing_rad.sin();
    let cos_b = bearing_rad.cos();

    // Fórmula de destino sobre el círculo máximo:
    //   φ2 = asin(sin φ1 · cos δ + cos φ1 · sin δ · cos θ)
    //   λ2 = λ1 + atan2(sin θ · sin δ · cos φ1, cos δ − sin φ1 · sin φ2)
    // El término de latitud lleva `cos θ` y el de longitud `sin θ`: intercambiarlos
    // produce un resultado plausible pero que va en la dirección equivocada, que es
    // la clase de error que no se ve en una prueba de humo.
    let sin_lat2 = (sin_lat * angular.cos() + cos_lat * angular.sin() * cos_b).asin();
    let lon2 = origin.lon.to_radians()
        + (sin_b * angular.sin() * cos_lat).atan2(angular.cos() - sin_lat * sin_lat2.sin());

    normalize(LatLon {
        lat: sin_lat2.to_degrees(),
        lon: lon2.to_degrees(),
    })
}

/// The point a fraction `t` of the way from `a` to `b` along the great circle.
#[must_use]
pub fn interpolate(a: LatLon, b: LatLon, t: f64) -> LatLon {
    let d = haversine_distance(a, b);
    if d == 0.0 {
        return a;
    }
    let clamped = t.clamp(0.0, 1.0);
    destination(a, bearing(a, b), d * clamped)
}

/// A local east-north-up tangent plane anchored at a reference coordinate.
///
/// This is the frame `gs-track`'s Kalman filter works in: the state is
/// `[e, n, v_e, v_n]` in metres, which keeps the matrices small and well
/// conditioned. `22` §2.3 requires the frame to be **re-anchored** when the
/// entity moves more than `track_frame_reanchor_m` from its origin, because a
/// tangent plane is only a good approximation near its anchor.
///
/// `Copy` and allocation-free.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LocalFrame {
    origin: LatLon,
    /// Metres per degree of longitude at the origin latitude.
    m_per_deg_lon: f64,
    /// Metres per degree of latitude at the origin latitude (spherical).
    m_per_deg_lat: f64,
}

impl LocalFrame {
    /// Anchors a frame at `origin`.
    #[must_use]
    pub fn new(origin: LatLon) -> Self {
        let lat_rad = origin.lat.to_radians();
        Self {
            origin,
            // `.abs()` porque `cos` se hace negativo al sur del ecuador, y un
            // factor de escala negativo haría que-going-east diese-steps-west.
            m_per_deg_lon: (EARTH_MEAN_RADIUS_M * lat_rad.cos() * std::f64::consts::PI / 180.0)
                .abs(),
            m_per_deg_lat: EARTH_MEAN_RADIUS_M * std::f64::consts::PI / 180.0,
        }
    }

    /// The anchor of the frame.
    #[must_use]
    pub const fn origin(&self) -> LatLon {
        self.origin
    }

    /// Metres per degree of longitude at this frame's latitude.
    ///
    /// Approaches zero towards the poles. That is physical, not a bug: a degree
    /// of longitude really is shorter there. It is why [`LocalFrame::to_enu`]
    /// is only a local approximation and why the frame must be re-anchored.
    #[must_use]
    pub const fn m_per_deg_lon(&self) -> f64 {
        self.m_per_deg_lon
    }

    /// Metres per degree of latitude at this frame's latitude.
    ///
    /// Derived from [`EARTH_MEAN_RADIUS_M`], like `m_per_deg_lon`, so the whole
    /// frame is **spherical and internally consistent** with
    /// [`haversine_distance`]. That consistency is the point: the tracking
    /// filter derives velocity from distances measured in this frame and must not
    /// mix a spherical frame with an ellipsoidal distance, or the error stops
    /// being a constant bias and becomes a function of direction.
    ///
    /// The cost is a known, bounded bias against the ellipsoid: on the equator
    /// the frame's north-south scale is 111 194.9 m/deg against the WGS84 meridian
    /// arc of 110 574.4 m/deg, i.e. **0.56 % over**. It is documented rather than
    /// corrected because `track_max_speed_mps` (70 m/s, `17` §2.11) has far more
    /// headroom than that, and because a frame that silently changed model would
    /// invalidate the checkpoints stored in the ephemeral store.
    #[must_use]
    pub const fn m_per_deg_lat(&self) -> f64 {
        self.m_per_deg_lat
    }

    /// Projects a coordinate into metres east/north of the anchor.
    ///
    /// The longitude difference is normalised to `[-180, 180)` first, so a track
    /// that crosses the antimeridian does not wrap to the other side of the
    /// planet.
    #[must_use]
    pub fn to_enu(&self, p: LatLon) -> (f64, f64) {
        let d_lon = (p.lon - self.origin.lon).rem_euclid(360.0);
        let d_lon = if d_lon > 180.0 { d_lon - 360.0 } else { d_lon };
        (
            d_lon * self.m_per_deg_lon,
            (p.lat - self.origin.lat) * self.m_per_deg_lat,
        )
    }

    /// Inverts [`LocalFrame::to_enu`]: metres east/north back to a coordinate.
    #[must_use]
    pub fn from_enu(&self, east_m: f64, north_m: f64) -> LatLon {
        normalize(LatLon {
            lat: self.origin.lat + north_m / self.m_per_deg_lat,
            lon: self.origin.lon + east_m / self.m_per_deg_lon,
        })
    }

    /// Distance between two coordinates within this frame, in metres.
    ///
    /// Cheaper than [`haversine_distance`] and exact enough for trackable
    /// pairs. **Not** valid for pairs far from the anchor, which is precisely
    /// what `track_frame_reanchor_m` bounds.
    #[must_use]
    pub fn planar_distance(&self, a: LatLon, b: LatLon) -> f64 {
        let (ae, an) = self.to_enu(a);
        let (be, bn) = self.to_enu(b);
        ((be - ae).powi(2) + (bn - an).powi(2)).sqrt()
    }
}

/// Normalises a longitude to `[-180, 180]` and clamps the latitude to the poles.
#[must_use]
fn normalize(mut p: LatLon) -> LatLon {
    if p.lon > 180.0 || p.lon < -180.0 {
        p.lon = p.lon.rem_euclid(360.0);
        if p.lon > 180.0 {
            p.lon -= 360.0;
        }
    }
    p.lat = p.lat.clamp(-90.0, 90.0);
    p
}

#[inline]
fn point(p: LatLon) -> Point<f64> {
    Point::new(p.lon, p.lat)
}

/// Total length of a polyline in metres (spherical model).
///
/// Returns `0.0` for fewer than two points. Duplicate consecutive points
/// contribute zero, so a track with repeated fixes is not double-counted.
#[must_use]
pub fn path_length_m(points: &[LatLon]) -> f64 {
    points
        .windows(2)
        .map(|w| haversine_distance(w[0], w[1]))
        .sum()
}

/// Ramer-Douglas-Peucker simplification with the tolerance in **metres**.
///
/// # Why not just call the `geo` implementation?
///
/// `geo::Simplify` works in the units of the geometry, and our geometry is
/// **degrees**. A tolerance expressed in degrees is not a distance: it is
/// latitude-dependent, because a degree of longitude shrinks towards the poles.
/// A 3 m tolerance near the equator is a 3 m tolerance; at 60° N it is 6 m, and
/// at 80° N it is 17 m. Simplifying a queue in Oslo with a tolerance tuned in
/// Panama silently throws away three times the detail.
///
/// So the polyline is projected into a [`LocalFrame`] (metres), simplified there,
/// and mapped back. `tolerance_m` means the same thing everywhere.
///
/// Complexity is O(n log n) typical, O(n²) worst case, and the result is
/// allocated once.
#[must_use]
pub fn simplify_rdp(points: &[LatLon], tolerance_m: f64) -> Vec<LatLon> {
    if points.len() < 3 {
        return points.to_vec();
    }
    // `!(x > 0.0)` y no `x <= 0.0`: también atrapa NaN, que debe conservar la
    // polilínea entera en vez de colapsarla a los extremos.
    if !tolerance_m.is_finite() || tolerance_m <= 0.0 {
        return points.to_vec();
    }

    let frame = LocalFrame::new(points[0]);
    let enu: Vec<(f64, f64)> = points.iter().map(|p| frame.to_enu(*p)).collect();

    let tol_sq = tolerance_m * tolerance_m;
    let mut keep = vec![false; points.len()];
    keep[0] = true;
    keep[points.len() - 1] = true;

    // Pila de segmentos (inicio, fin) por procesar. Explícitamente iterativa: la
    // versión recursiva desborda la pila con trazas largas de miles de puntos.
    let mut stack: Vec<(usize, usize)> = vec![(0, points.len() - 1)];

    while let Some((first, last)) = stack.pop() {
        if last <= first + 1 {
            continue;
        }
        let (ax, ay) = enu[first];
        let (bx, by) = enu[last];

        let mut max_dist_sq = 0.0;
        let mut max_index = first;
        for (i, (px, py)) in enu.iter().copied().enumerate().take(last).skip(first + 1) {
            let dist_sq = perpendicular_distance_sq(px, py, ax, ay, bx, by);
            if dist_sq > max_dist_sq {
                max_dist_sq = dist_sq;
                max_index = i;
            }
        }

        if max_dist_sq > tol_sq {
            keep[max_index] = true;
            stack.push((first, max_index));
            stack.push((max_index, last));
        }
    }

    points
        .iter()
        .zip(keep)
        .filter_map(|(p, k)| k.then_some(*p))
        .collect()
}

/// Squared distance from `(px, py)` to the segment `a -> b`.
///
/// Returns the squared distance to the segment (not to the infinite line), which
/// is what RDP requires: a point beyond the end of the segment must be measured
/// to the end, not to the extrapolated line.
fn perpendicular_distance_sq(px: f64, py: f64, ax: f64, ay: f64, bx: f64, by: f64) -> f64 {
    let dx = bx - ax;
    let dy = by - ay;
    let len_sq = dx * dx + dy * dy;

    if len_sq <= f64::MIN_POSITIVE {
        // Segmento degenerado: distancia al propio punto.
        let ex = px - ax;
        let ey = py - ay;
        return ex * ex + ey * ey;
    }

    // Parámetro t del punto proyectado, acotado a [0, 1].
    let t = (((px - ax) * dx + (py - ay) * dy) / len_sq).clamp(0.0, 1.0);
    let cx = ax + t * dx;
    let cy = ay + t * dy;
    let ex = px - cx;
    let ey = py - cy;
    ex * ex + ey * ey
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Assertion helper with an absolute tolerance in metres or degrees.
    fn assert_close(actual: f64, expected: f64, tol: f64, what: &str) {
        assert!(
            (actual - expected).abs() <= tol,
            "{what}: got {actual}, expected {expected} (±{tol})"
        );
    }

    /// Difference between two angles in degrees, taking the shorter way round.
    ///
    /// Un rumbo de 359,9999999992° y otro de 0° son el mismo rumbo, pero su
    /// diferencia aritmética es 360°. Comparar angulos con `assert_close`
    /// produciría un fallo falso justo en el redondeo del punto cardinal norte,
    /// que es donde las pruebas de bearing más se ejecutan.
    fn assert_bearing_close(actual: f64, expected: f64, tol: f64, what: &str) {
        let diff = (actual - expected).rem_euclid(360.0);
        let diff = if diff > 180.0 { 360.0 - diff } else { diff };
        assert!(
            diff <= tol,
            "{what}: got {actual}°, expected {expected}° (±{tol}°, circular)"
        );
    }

    // ---- Constantes -------------------------------------------------------

    #[test]
    fn constants_match_their_documented_sources() {
        // docs/17 §2.11. These are the values the documentation promises, so a
        // change here is a change to the contract and must be deliberate.
        assert_eq!(EARTH_MEAN_RADIUS_M, 6_371_008.8);
        assert_eq!(EARTH_SEMIMAJOR_AXIS_M, 6_378_137.0);
        assert_close(EARTH_FLATTENING, 1.0 / 298.257_223_563, 1e-18, "flattening");
    }

    #[test]
    fn haversine_agrees_with_the_geo_crate() {
        // We re-declare the radius instead of using `geo::Haversine`, so the two
        // must agree exactly; if they diverge, our constant has drifted from the
        // one the library uses.
        let a = LatLon::new(8.9824, -79.5199);
        let b = LatLon::new(40.7128, -74.0060);
        assert_eq!(
            haversine_distance(a, b),
            geo::Haversine.distance(point(a), point(b))
        );
    }

    // ---- LatLon -----------------------------------------------------------

    #[test]
    fn latlon_validation() {
        assert!(LatLon::new(0.0, 0.0).is_valid());
        assert!(LatLon::new(90.0, 180.0).is_valid());
        assert!(LatLon::new(-90.0, -180.0).is_valid());
        assert!(!LatLon::new(90.1, 0.0).is_valid());
        assert!(!LatLon::new(-90.1, 0.0).is_valid());
        assert!(!LatLon::new(0.0, 180.1).is_valid());
        assert!(!LatLon::new(f64::NAN, 0.0).is_valid());
        assert!(!LatLon::new(0.0, f64::INFINITY).is_valid());
    }

    // ---- Distancia --------------------------------------------------------

    #[test]
    fn distance_to_self_is_zero() {
        let p = LatLon::new(8.9824, -79.5199);
        assert_eq!(haversine_distance(p, p), 0.0);
        assert_eq!(geodesic_distance(p, p).meters, 0.0);
    }

    #[test]
    fn distance_is_symmetric() {
        let a = LatLon::new(8.9824, -79.5199);
        let b = LatLon::new(48.8566, 2.3522);
        assert_close(
            haversine_distance(a, b),
            haversine_distance(b, a),
            1e-6,
            "symmetry (haversine)",
        );
        assert_close(
            geodesic_distance(a, b).meters,
            geodesic_distance(b, a).meters,
            1e-6,
            "symmetry (geodesic)",
        );
    }

    #[test]
    fn one_degree_of_latitude_is_the_meridian_arc() {
        // Valor geodésico conocido del arco meridiano WGS84 para 1° de latitud
        // en el ecuador: ~110 574 m. Es una comprobación INDEPENDIENTE de la
        // fórmula: si el radio o los ejes estuvieran mal, esto no cuadraría.
        let d = geodesic_distance(LatLon::new(0.0, 0.0), LatLon::new(1.0, 0.0));
        assert_eq!(d.method, DistanceMethod::Vincenty);
        assert_close(d.meters, 110_574.4, 1.0, "meridian arc 1°");
    }

    #[test]
    fn one_degree_of_longitude_at_the_equator_is_the_parallel_arc() {
        // Arco de paralelo WGS84 en el ecuador: a * π/180 = 111 319.5 m.
        let d = geodesic_distance(LatLon::new(0.0, 0.0), LatLon::new(0.0, 1.0));
        assert_eq!(d.method, DistanceMethod::Vincenty);
        assert_close(d.meters, 111_319.5, 1.0, "parallel arc 1° at equator");
    }

    #[test]
    fn the_two_models_differ_by_a_direction_dependent_ratio_at_every_scale() {
        // Ésta es la invariante real, y es más fuerte que «se parecen a corta
        // distancia»: la diferencia entre esfera y elipsoide **no depende de la
        // escala sino de la DIRECCIÓN**, y se predice desde los parámetros.
        //
        //   · Norte-sur en el ecuador manda el radio de curvatura del meridiano,
        //     M = a·(1−f)², que es MENOR que el radio medio → esférico > geodésico.
        //   · Este-oeste en el ecuador manda el semieje mayor, a > R → geodésico > esférico.
        //
        // Estas aserciones fallarían si alguien cambiara el elipsoide, el radio
        // medio, o intercambiara los términos de latitud y longitud de `destination`.
        let f = EARTH_FLATTENING;
        let meridian_radius = EARTH_SEMIMAJOR_AXIS_M * (1.0 - f) * (1.0 - f);
        let expected_north = meridian_radius / EARTH_MEAN_RADIUS_M;
        let expected_east = EARTH_SEMIMAJOR_AXIS_M / EARTH_MEAN_RADIUS_M;

        assert!(
            expected_north < 1.0,
            "M < R: a meridian degree is shorter than a mean-radius degree"
        );
        assert!(
            expected_east > 1.0,
            "a > R: an equatorial parallel degree is longer than a mean-radius degree"
        );

        // A 10 m y a 100 km el ratio es el mismo: por eso son dos aserciones y no
        // una prueba de «converge a corta distancia».
        for meters in [10.0_f64, 1_000.0, 100_000.0] {
            let north_to = LatLon::new(meters / 110_574.0, 0.0);
            let h_n = haversine_distance(LatLon::new(0.0, 0.0), north_to);
            let g_n = geodesic_distance(LatLon::new(0.0, 0.0), north_to).meters;
            assert_close(
                g_n / h_n,
                expected_north,
                1e-5,
                "north-south ratio must be M/R",
            );

            let east_to = LatLon::new(0.0, meters / 111_319.4907932264);
            let h_e = haversine_distance(LatLon::new(0.0, 0.0), east_to);
            let g_e = geodesic_distance(LatLon::new(0.0, 0.0), east_to).meters;
            assert_close(
                g_e / h_e,
                expected_east,
                1e-5,
                "east-west ratio must be a/R",
            );
        }
    }

    #[test]
    fn a_parallel_degree_shrinks_with_latitude() {
        let equator = geodesic_distance(LatLon::new(0.0, 0.0), LatLon::new(0.0, 1.0)).meters;
        let sixty = geodesic_distance(LatLon::new(60.0, 0.0), LatLon::new(60.0, 1.0)).meters;
        let eighty = geodesic_distance(LatLon::new(80.0, 0.0), LatLon::new(80.0, 1.0)).meters;

        assert!(sixty < equator, "60° must be shorter than the equator");
        assert!(eighty < sixty, "80° must be shorter than 60°");
        // El paralelo del elipsoide NO es exactamente `cos(φ)` veces el del
        // ecuador: ν = a/sqrt(1−e²sin²φ) también crece con la latitud, así que el
        // valor real queda un poco por encima de `equador · cos φ`. Por eso la
        // aserción va con margen y no con igualdad exacta.
        assert_close(sixty, equator * 0.5, 150.0, "60° N parallel arc");
        // Y por eso no se afirma contra un valor de tabla que el repositorio no
        // trae: el arco paralelo exacto del WGS84 necesita la serie completa, y
        // un número copiado de memoria es peor que una cota derivable.
    }

    #[test]
    fn the_ellipsoidal_model_stays_within_half_a_percent_of_the_spherical_one() {
        // Cota del sesgo entre modelos, para que nadie lo use sin saber cuánto
        // vale: ~0,56 %, que es la diferencia entre M y a (6 335 439 / 6 378 137).
        let a = LatLon::new(8.9824, -79.5199);
        for b in [
            LatLon::new(8.9834, -79.5209),
            LatLon::new(9.0000, -79.4900),
            LatLon::new(8.9600, -79.5600),
            LatLon::new(51.5074, -0.1278),
            LatLon::new(-33.4489, -70.6693),
        ] {
            let h = haversine_distance(a, b);
            let g = geodesic_distance(a, b).meters;
            let err = (g - h).abs() / h;
            assert!(
                err < 0.006,
                "model gap for {b:?} is {err}, above the 0.6% bound"
            );
        }
    }

    #[test]
    fn near_antipodal_pairs_report_the_fallback_instead_of_lying() {
        // Vincenty no converge cerca del antípoda. El número que se devuelve
        // sirve, pero el método lo dice; un caller que exija precisión elipsoidal
        // tiene que poder detectarlo en vez de creer que la tuvo.
        let a = LatLon::new(0.0, 0.0);
        let b = LatLon::new(0.5, 179.7);
        let d = geodesic_distance(a, b);
        assert_eq!(d.method, DistanceMethod::HaversineFallback);
        // Casi media circunferencia: medio mundo. Una aserción «está en un rango»
        // sin decir cuál habría dejado pasar un cero o un 40 Mm.
        assert_close(d.meters, 19_950_277.0, 5_000.0, "near-antipodal distance");
    }

    #[test]
    fn geodesic_distance_is_symmetric_including_the_method() {
        let a = LatLon::new(0.0, 0.0);
        let b = LatLon::new(0.5, 179.7);
        assert_eq!(
            geodesic_distance(a, b).method,
            geodesic_distance(b, a).method,
            "the fallback must trigger symmetrically"
        );
    }

    // ---- Rumbo ------------------------------------------------------------

    #[test]
    fn bearing_cardinals() {
        let origin = LatLon::new(0.0, 0.0);
        assert_bearing_close(
            bearing(origin, LatLon::new(1.0, 0.0)),
            0.0,
            1e-6,
            "due north",
        );
        assert_bearing_close(
            bearing(origin, LatLon::new(0.0, 1.0)),
            90.0,
            1e-6,
            "due east",
        );
        assert_bearing_close(
            bearing(origin, LatLon::new(-1.0, 0.0)),
            180.0,
            1e-6,
            "due south",
        );
        assert_bearing_close(
            bearing(origin, LatLon::new(0.0, -1.0)),
            270.0,
            1e-6,
            "due west",
        );
    }

    #[test]
    fn bearing_stays_in_zero_to_360() {
        let a = LatLon::new(50.0, 10.0);
        for b in [
            LatLon::new(-50.0, -170.0),
            LatLon::new(10.0, 170.0),
            LatLon::new(-80.0, 5.0),
        ] {
            let brg = bearing(a, b);
            assert!(
                (0.0..360.0).contains(&brg),
                "bearing out of range: {brg} for {b:?}"
            );
        }
    }

    #[test]
    fn bearing_to_the_same_point_is_zero_not_nan() {
        // Un NaN aquí envenenaría el estado del filtro para todas las muestras
        // posteriores. Cero es una respuesta degradada pero inocua.
        let p = LatLon::new(8.9824, -79.5199);
        let brg = bearing(p, p);
        assert!(brg.is_finite());
        assert_eq!(brg, 0.0);
    }

    // ---- Destination / interpolate ---------------------------------------

    #[test]
    fn destination_round_trips_through_bearing_and_distance() {
        let origin = LatLon::new(8.9824, -79.5199);
        for (brg, dist) in [(0.0, 100.0), (90.0, 250.0), (217.0, 1000.0), (359.0, 50.0)] {
            let p = destination(origin, brg, dist);
            assert_bearing_close(
                bearing(origin, p),
                brg,
                0.01,
                &format!("bearing after destination({brg}, {dist})"),
            );
            assert_close(
                haversine_distance(origin, p),
                dist,
                dist * 0.001,
                &format!("distance after destination({brg}, {dist})"),
            );
        }
    }

    #[test]
    fn destination_of_zero_distance_is_the_origin() {
        let origin = LatLon::new(8.9824, -79.5199);
        assert_eq!(destination(origin, 123.0, 0.0), origin);
    }

    #[test]
    fn destination_normalises_the_longitude() {
        // Al este del antimeridiana: la longitud debe quedar en [-180, 180], no
        // en 180.5. Si no, el siguiente salto demás de 180° se convierte en un
        // salto de 359°.
        let near_dateline = LatLon::new(0.0, 179.999);
        let p = destination(near_dateline, 90.0, 1000.0);
        assert!(
            (-180.0..=180.0).contains(&p.lon),
            "longitude out of range after crossing the antimeridian: {}",
            p.lon
        );
    }

    #[test]
    fn interpolate_endpoints_and_midpoint() {
        let a = LatLon::new(8.9824, -79.5199);
        let b = LatLon::new(8.9900, -79.5199);
        assert_eq!(interpolate(a, b, 0.0), a);
        assert_close(interpolate(a, b, 1.0).lat, b.lat, 1e-9, "t=1");
        let mid = interpolate(a, b, 0.5);
        let half = haversine_distance(a, b) / 2.0;
        assert_close(haversine_distance(a, mid), half, 0.5, "halfway point");
        assert_close(haversine_distance(mid, b), half, 0.5, "second half");
    }

    #[test]
    fn interpolate_clamps_fraction_outside_zero_one() {
        let a = LatLon::new(0.0, 0.0);
        let b = LatLon::new(0.0, 1.0);
        assert_eq!(interpolate(a, b, -1.0), a);
        assert_close(
            interpolate(a, b, 5.0).lon,
            1.0,
            1e-9,
            "t>1 clamps to the end",
        );
    }

    #[test]
    fn interpolate_of_identical_points_is_that_point() {
        let p = LatLon::new(8.9824, -79.5199);
        assert_eq!(interpolate(p, p, 0.5), p);
    }

    // ---- LocalFrame -------------------------------------------------------

    #[test]
    fn local_frame_origin_maps_to_zero() {
        let origin = LatLon::new(8.9824, -79.5199);
        let frame = LocalFrame::new(origin);
        let (e, n) = frame.to_enu(origin);
        assert_eq!(e, 0.0);
        assert_eq!(n, 0.0);
    }

    #[test]
    fn local_frame_round_trips() {
        let frame = LocalFrame::new(LatLon::new(8.9824, -79.5199));
        for p in [
            LatLon::new(8.9900, -79.5199),
            LatLon::new(8.9824, -79.5100),
            LatLon::new(9.0000, -79.5300),
        ] {
            let (e, n) = frame.to_enu(p);
            let back = frame.from_enu(e, n);
            assert_close(back.lat, p.lat, 1e-9, "lat round trip");
            assert_close(back.lon, p.lon, 1e-9, "lon round trip");
        }
    }

    #[test]
    fn local_frame_east_is_positive_and_north_is_positive() {
        let frame = LocalFrame::new(LatLon::new(0.0, 0.0));
        let (e, n) = frame.to_enu(LatLon::new(0.5, 0.5));
        assert!(e > 0.0, "east must be positive");
        assert!(n > 0.0, "north must be positive");
        let (e, n) = frame.to_enu(LatLon::new(-0.5, -0.5));
        assert!(e < 0.0 && n < 0.0);
    }

    #[test]
    fn local_frame_handles_the_antimeridian() {
        let frame = LocalFrame::new(LatLon::new(0.0, 179.999));
        let (e, _n) = frame.to_enu(LatLon::new(0.0, -179.999));
        assert!(
            e > 0.0 && e < 10_000.0,
            "crossing the antimeridian must be a short step east, got {e} m"
        );
    }

    #[test]
    fn local_frame_metres_per_degree_shrinks_with_latitude() {
        let equator = LocalFrame::new(LatLon::new(0.0, 0.0)).m_per_deg_lon();
        let sixty = LocalFrame::new(LatLon::new(60.0, 0.0)).m_per_deg_lon();
        assert_close(
            sixty,
            equator * 0.5,
            1.0,
            "60° N metres per longitude degree",
        );
        // Metres per degree of latitude does not vary with latitude in the
        // spherical model; documenting it so nobody "fixes" it later.
        assert_eq!(
            LocalFrame::new(LatLon::new(0.0, 0.0)).m_per_deg_lat(),
            LocalFrame::new(LatLon::new(60.0, 0.0)).m_per_deg_lat()
        );
    }

    #[test]
    fn local_frame_planar_distance_is_close_to_haversine_near_the_anchor() {
        let origin = LatLon::new(8.9824, -79.5199);
        let frame = LocalFrame::new(origin);
        for p in [
            LatLon::new(8.9900, -79.5199),
            LatLon::new(8.9824, -79.5100),
            LatLon::new(8.9700, -79.4900),
        ] {
            let planar = frame.planar_distance(origin, p);
            let spherical = haversine_distance(origin, p);
            let err = (planar - spherical).abs() / spherical;
            assert!(err < 0.002, "planar vs spherical error {err} for {p:?}");
        }
    }

    // ---- Polyline ---------------------------------------------------------

    #[test]
    fn path_length_of_short_polyline() {
        let points = [
            LatLon::new(0.0, 0.0),
            LatLon::new(0.0, 0.001),
            LatLon::new(0.0, 0.002),
        ];
        // 0.002° de longitud en el ecuador = 0.002 * 111 319.5 = 222.639 m
        assert_close(path_length_m(&points), 222.639, 0.5, "polyline length");
    }

    #[test]
    fn path_length_of_trivial_polylines_is_zero() {
        assert_eq!(path_length_m(&[]), 0.0);
        assert_eq!(path_length_m(&[LatLon::new(0.0, 0.0)]), 0.0);
        let p = LatLon::new(1.0, 2.0);
        assert_eq!(path_length_m(&[p, p]), 0.0);
    }

    #[test]
    fn simplify_keeps_endpoints_of_a_straight_line() {
        let a = LatLon::new(8.9824, -79.5199);
        let b = LatLon::new(8.9900, -79.5199);
        let mut straight = vec![a];
        for i in 1..20 {
            let t = f64::from(i) / 20.0;
            straight.push(LatLon::new(
                a.lat + (b.lat - a.lat) * t,
                a.lon + (b.lon - a.lon) * t,
            ));
        }
        straight.push(b);

        let simplified = simplify_rdp(&straight, 3.0);
        assert_eq!(
            simplified,
            vec![a, b],
            "a straight line must collapse to its endpoints"
        );
    }

    #[test]
    fn simplify_keeps_a_detour_beyond_the_tolerance() {
        let a = LatLon::new(0.0, 0.0);
        let b = LatLon::new(0.0, 0.01);
        // Punto intermedio a 200 m al norte: muy por encima de una tolerancia de 3 m.
        let detour = LatLon::new(200.0 / 111_195.0, 0.005);
        let points = vec![a, detour, b];

        let simplified = simplify_rdp(&points, 3.0);
        assert_eq!(
            simplified, points,
            "a 200 m detour must survive a 3 m tolerance"
        );
    }

    #[test]
    fn simplify_drops_a_detour_below_the_tolerance() {
        let a = LatLon::new(0.0, 0.0);
        let b = LatLon::new(0.0, 0.01);
        // 1 m de desvío con tolerancia de 3 m: no aporta información.
        let tiny = LatLon::new(1.0 / 111_195.0, 0.005);
        let simplified = simplify_rdp(&[a, tiny, b], 3.0);
        assert_eq!(simplified, vec![a, b]);
    }

    #[test]
    fn simplify_tolerance_is_in_metres_at_any_latitude() {
        // El motivo de proyectar a un marco local: una tolerancia en grados NO es
        // una distancia. A 64° N un grado de longitud mide la mitad que en el
        // ecuador, así que una tolerancia «de 3 grados» equivaldría a ~2 m aquí y
        // a ~1 m en el ecuador.
        let lat = 64.0;
        let a = LatLon::new(lat, 0.0);
        let b = LatLon::new(lat, 0.001);
        // Desvío de 10 m al norte, en el medio.
        let detour = LatLon::new(lat + 10.0 / 111_195.0, 0.0005);
        let points = vec![a, detour, b];

        // Con 3 m de tolerancia el desvío de 10 m debe sobrevivir…
        let kept = simplify_rdp(&points, 3.0);
        assert_eq!(kept, points, "10 m detour must survive a 3 m tolerance");

        // …y con 50 m debe desaparecer.
        let dropped = simplify_rdp(&points, 50.0);
        assert_eq!(
            dropped,
            vec![a, b],
            "10 m detour must go with a 50 m tolerance"
        );
    }

    #[test]
    fn simplify_returns_input_for_short_or_zero_tolerance() {
        let points = vec![LatLon::new(0.0, 0.0), LatLon::new(0.0, 0.01)];
        assert_eq!(simplify_rdp(&points, 3.0), points);
        let single = vec![LatLon::new(1.0, 1.0)];
        assert_eq!(simplify_rdp(&single, 3.0), single);
        assert_eq!(simplify_rdp(&[], 3.0), Vec::new());

        let three = vec![
            LatLon::new(0.0, 0.0),
            LatLon::new(0.001, 0.005),
            LatLon::new(0.0, 0.01),
        ];
        assert_eq!(
            simplify_rdp(&three, 0.0),
            three,
            "zero tolerance must keep everything"
        );
        assert_eq!(
            simplify_rdp(&three, f64::NAN),
            three,
            "NaN tolerance must keep everything"
        );
    }

    #[test]
    fn simplify_never_exceeds_the_tolerance_of_the_removed_points() {
        // Propiedad: tras simplificar, la distancia de cada punto eliminado a la
        // polilínea resultante no supera la tolerancia. Es la garantía que da
        // sentido a simplificar en línea.
        let origin = LatLon::new(40.0, -3.0);
        let frame = LocalFrame::new(origin);
        let mut points = Vec::new();
        for i in 0..500 {
            let t = f64::from(i) / 500.0;
            // Ruido determinista (sin `rand`): dos senos de distinta frecuencia.
            let north = 20.0 * (t * 40.0).sin() + 4.0 * (t * 131.0).sin();
            let east = 1000.0 * t;
            points.push(frame.from_enu(east, north));
        }

        let tolerance = 5.0;
        let simplified = simplify_rdp(&points, tolerance);

        assert!(simplified.len() < points.len(), "must actually simplify");
        for p in &points {
            // Distancia mínima a la polilínea simplificada.
            let mut best = f64::INFINITY;
            for w in simplified.windows(2) {
                let d = segment_distance(*p, w[0], w[1]);
                best = best.min(d);
            }
            assert!(
                best <= tolerance * 1.001,
                "point {p:?} is {best} m from the simplified polyline, tolerance {tolerance}"
            );
        }
    }

    /// Distance in metres from `p` to the segment `a -> b`, spherical.
    fn segment_distance(p: LatLon, a: LatLon, b: LatLon) -> f64 {
        let frame = LocalFrame::new(a);
        let (px, py) = frame.to_enu(p);
        let (ax, ay) = (0.0, 0.0);
        let (bx, by) = frame.to_enu(b);
        let dist_sq = perpendicular_distance_sq(px, py, ax, ay, bx, by);
        dist_sq.sqrt()
    }
}

/// H3 cell helpers (ADR-0026), behind the `h3` feature.
///
/// Edge lengths and areas are **queried from `h3o` at runtime**, never hardcoded
/// (ADR-0026): a constant copied by hand drifts from the library and nobody
/// notices.
#[cfg(feature = "h3")]
pub mod h3 {
    use super::LatLon;

    /// The H3 cell that contains `point` at `resolution` (0..=15).
    #[must_use]
    pub fn cell_of(point: LatLon, resolution: u8) -> Option<u64> {
        let resolution = h3o::Resolution::try_from(u8::min(resolution, 15)).ok()?;
        let latlng = h3o::LatLng::new(point.lat, point.lon).ok()?;
        Some(u64::from(latlng.to_cell(resolution)))
    }

    /// The centre of an H3 cell.
    #[must_use]
    pub fn cell_center(cell: u64) -> Option<LatLon> {
        let index = h3o::CellIndex::try_from(cell).ok()?;
        let latlng: h3o::LatLng = index.into();
        Some(LatLon::new(latlng.lat(), latlng.lng()))
    }

    /// Average edge length of the cells at `resolution`, in metres.
    #[must_use]
    pub fn cell_edge_length_m(resolution: u8) -> Option<f64> {
        let resolution = h3o::Resolution::try_from(u8::min(resolution, 15)).ok()?;
        Some(resolution.area_km2().sqrt() * 1000.0 / 2.0)
    }

    /// The resolution of a cell (0..=15).
    #[must_use]
    pub fn cell_resolution(cell: u64) -> Option<u8> {
        let index = h3o::CellIndex::try_from(cell).ok()?;
        Some(u8::from(index.resolution()))
    }

    /// Canonical hex representation of a cell (15 or 16 digits).
    #[must_use]
    pub fn cell_hex(cell: u64) -> String {
        format!("{cell:015x}")
    }

    /// Whether `cell` is contained in the axis-aligned bounding box
    /// `[min_lat, max_lat] x [min_lon, max_lon]`, by its centre.
    #[must_use]
    pub fn cell_in_bbox(cell: u64, min_lat: f64, min_lon: f64, max_lat: f64, max_lon: f64) -> bool {
        match cell_center(cell) {
            Some(c) => c.lat >= min_lat && c.lat <= max_lat && c.lon >= min_lon && c.lon <= max_lon,
            None => false,
        }
    }
}

#[cfg(all(test, feature = "h3"))]
mod h3_tests {
    use super::h3;

    #[test]
    fn cell_round_trip_contains_the_point() {
        let p = super::LatLon::new(8.9824, -79.5199);
        let cell = h3::cell_of(p, 9).expect("cell");
        let center = h3::cell_center(cell).expect("center");
        // The centre is within one cell edge of the point.
        let d = super::haversine_distance(p, center);
        let edge = h3::cell_edge_length_m(9).expect("edge");
        assert!(d < edge * 3.0, "centre too far: {d} m vs edge {edge} m");
    }

    #[test]
    fn cell_hex_has_the_canonical_length() {
        let cell = h3::cell_of(super::LatLon::new(8.9824, -79.5199), 9).unwrap();
        let hex = h3::cell_hex(cell);
        assert!((15..=16).contains(&hex.len()), "hex was {hex}");
        assert!(hex.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn bbox_filter_uses_the_centre() {
        let cell = h3::cell_of(super::LatLon::new(8.9824, -79.5199), 9).unwrap();
        assert!(h3::cell_in_bbox(cell, 8.0, -80.0, 9.0, -79.0));
        assert!(!h3::cell_in_bbox(cell, 40.0, -80.0, 41.0, -79.0));
    }

    #[test]
    fn edge_length_is_queried_not_hardcoded() {
        // The H3 average edge length at resolution 9 is ~174 m; the point is that
        // it comes from the library, so this only asserts plausibility.
        let edge = h3::cell_edge_length_m(9).unwrap();
        assert!((50.0..=500.0).contains(&edge), "edge was {edge}");
    }
}
