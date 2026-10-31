/// Geodesia mínima para el editor de geocercas y su evaluación local.
///
/// Reproduce, en Dart, las fórmulas que `docs/14_GEOFENCING.md` §4 fija para
/// el servidor (haversine y proyección equirectangular local). Es una vista
/// previa: la transición canónica la decide el servidor (`14` §5.1).
library;

import 'dart:math' as math;

/// Radio terrestre medio en metros (IUGG), el mismo que usa `gs-geo`.
const double earthRadiusM = 6371008.8;

/// Coordenada WGS84 en grados, con el orden de la API (`{latitude, longitude}`).
final class GeoPoint {
  /// Crea un punto; no valida rangos (ver [isValid]).
  const GeoPoint(this.latitude, this.longitude);

  /// Latitud en grados, -90..=90.
  final double latitude;

  /// Longitud en grados, -180..=180.
  final double longitude;

  /// Whether las dos coordenadas son finitas y están en rango (`09` §3).
  bool get isValid =>
      latitude.isFinite &&
      longitude.isFinite &&
      latitude >= -90 &&
      latitude <= 90 &&
      longitude >= -180 &&
      longitude <= 180;

  /// Objeto `LatLng` del contrato.
  Map<String, double> toJson() => {
    'latitude': latitude,
    'longitude': longitude,
  };

  /// Lee un `LatLng` del contrato; lanza [FormatException] si falta un campo.
  static GeoPoint fromJson(Map<String, dynamic> json) {
    final lat = json['latitude'];
    final lon = json['longitude'];
    if (lat is! num || lon is! num) {
      throw const FormatException('LatLng sin latitude/longitude numéricos');
    }
    return GeoPoint(lat.toDouble(), lon.toDouble());
  }

  @override
  bool operator ==(Object other) =>
      other is GeoPoint &&
      other.latitude == latitude &&
      other.longitude == longitude;

  @override
  int get hashCode => Object.hash(latitude, longitude);
}

double _rad(double deg) => deg * math.pi / 180;
double _deg(double rad) => rad * 180 / math.pi;

/// Distancia de círculo máximo en metros entre [a] y [b].
double haversineM(GeoPoint a, GeoPoint b) {
  final dLat = _rad(b.latitude - a.latitude);
  final dLon = _rad(b.longitude - a.longitude);
  final h =
      math.pow(math.sin(dLat / 2), 2) +
      math.cos(_rad(a.latitude)) *
          math.cos(_rad(b.latitude)) *
          math.pow(math.sin(dLon / 2), 2);
  return 2 * earthRadiusM * math.asin(math.min(1, math.sqrt(h)));
}

/// Punto a [distanceM] metros de [origin] con rumbo [bearingDeg].
GeoPoint destination(GeoPoint origin, double bearingDeg, double distanceM) {
  final d = distanceM / earthRadiusM;
  final b = _rad(bearingDeg);
  final lat1 = _rad(origin.latitude);
  final lon1 = _rad(origin.longitude);
  final lat2 = math.asin(
    math.sin(lat1) * math.cos(d) + math.cos(lat1) * math.sin(d) * math.cos(b),
  );
  final lon2 =
      lon1 +
      math.atan2(
        math.sin(b) * math.sin(d) * math.cos(lat1),
        math.cos(d) - math.sin(lat1) * math.sin(lat2),
      );
  final lon = (_deg(lon2) + 540) % 360 - 180;
  return GeoPoint(_deg(lat2), lon);
}

/// Anillo cerrado de [segments] lados que aproxima un círculo, para dibujarlo.
List<GeoPoint> circleRing(
  GeoPoint center,
  double radiusM, {
  int segments = 64,
}) {
  final ring = [
    for (var i = 0; i < segments; i++)
      destination(center, 360 * i / segments, radiusM),
  ];
  return [...ring, ring.first];
}

/// Proyección equirectangular local en metros, centrada en [origin].
({double x, double y}) _project(GeoPoint origin, GeoPoint p) {
  final x =
      _rad(p.longitude - origin.longitude) *
      math.cos(_rad(origin.latitude)) *
      earthRadiusM;
  final y = _rad(p.latitude - origin.latitude) * earthRadiusM;
  return (x: x, y: y);
}

double _segmentDistance(
  ({double x, double y}) p,
  ({double x, double y}) a,
  ({double x, double y}) b,
) {
  final dx = b.x - a.x;
  final dy = b.y - a.y;
  final len2 = dx * dx + dy * dy;
  var t = len2 == 0 ? 0.0 : ((p.x - a.x) * dx + (p.y - a.y) * dy) / len2;
  t = t.clamp(0.0, 1.0);
  final cx = a.x + t * dx - p.x;
  final cy = a.y + t * dy - p.y;
  return math.sqrt(cx * cx + cy * cy);
}

/// Whether [p] cae dentro del anillo cerrado [ring] (regla par-impar).
bool pointInRing(GeoPoint p, List<GeoPoint> ring) {
  var inside = false;
  for (var i = 0, j = ring.length - 1; i < ring.length; j = i++) {
    final a = ring[i];
    final b = ring[j];
    final crosses =
        (a.latitude > p.latitude) != (b.latitude > p.latitude) &&
        p.longitude <
            (b.longitude - a.longitude) *
                    (p.latitude - a.latitude) /
                    (b.latitude - a.latitude) +
                a.longitude;
    if (crosses) inside = !inside;
  }
  return inside;
}

/// Distancia firmada en metros de [p] al borde de [ring]: negativa dentro.
///
/// Usa la proyección equirectangular local centrada en [p] (`14` §4 paso 2).
double signedDistanceToRingM(GeoPoint p, List<GeoPoint> ring) {
  final origin = (x: 0.0, y: 0.0);
  var best = double.infinity;
  for (var i = 0; i + 1 < ring.length; i++) {
    final d = _segmentDistance(
      origin,
      _project(p, ring[i]),
      _project(p, ring[i + 1]),
    );
    if (d < best) best = d;
  }
  return pointInRing(p, ring) ? -best : best;
}

/// Área con signo en m² del anillo [ring] proyectado alrededor de su primer punto.
double ringAreaM2(List<GeoPoint> ring) {
  if (ring.length < 4) return 0;
  final o = ring.first;
  var sum = 0.0;
  for (var i = 0; i + 1 < ring.length; i++) {
    final a = _project(o, ring[i]);
    final b = _project(o, ring[i + 1]);
    sum += a.x * b.y - b.x * a.y;
  }
  return (sum / 2).abs();
}

bool _segmentsIntersect(
  ({double x, double y}) p1,
  ({double x, double y}) p2,
  ({double x, double y}) p3,
  ({double x, double y}) p4,
) {
  double cross(
    ({double x, double y}) a,
    ({double x, double y}) b,
    ({double x, double y}) c,
  ) => (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
  final d1 = cross(p3, p4, p1);
  final d2 = cross(p3, p4, p2);
  final d3 = cross(p1, p2, p3);
  final d4 = cross(p1, p2, p4);
  return ((d1 > 0 && d2 < 0) || (d1 < 0 && d2 > 0)) &&
      ((d3 > 0 && d4 < 0) || (d3 < 0 && d4 > 0));
}

/// Whether algún par de aristas no adyacentes del anillo cerrado se cruza.
bool ringSelfIntersects(List<GeoPoint> ring) {
  if (ring.length < 5) return false;
  final o = ring.first;
  final pts = [for (final p in ring) _project(o, p)];
  final n = pts.length - 1; // número de aristas
  for (var i = 0; i < n; i++) {
    for (var j = i + 1; j < n; j++) {
      final adjacent = j == i + 1 || (i == 0 && j == n - 1);
      if (adjacent) continue;
      if (_segmentsIntersect(pts[i], pts[i + 1], pts[j], pts[j + 1])) {
        return true;
      }
    }
  }
  return false;
}
