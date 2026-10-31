/// Dibujo de geocercas y posiciones sobre un mapa MapLibre.
library;

import 'package:maplibre_gl/maplibre_gl.dart';

import '../geo/geodesy.dart';
import '../geo/geofence.dart';
import '../geo/geofence_evaluator.dart';

/// Convierte a la coordenada de MapLibre.
LatLng toLatLng(GeoPoint p) => LatLng(p.latitude, p.longitude);

/// Anillo dibujable de [shape] (los círculos se aproximan con 64 lados).
List<GeoPoint> drawableRing(GeofenceShape shape) => switch (shape) {
  CircleShape(:final center, :final radiusM) => circleRing(center, radiusM),
  PolygonShape(:final ring) => ring,
};

/// Color de relleno según el estado local.
String fenceColor(FenceState state) => switch (state) {
  FenceState.inside => '#2e7d32',
  FenceState.outside => '#1565c0',
  FenceState.unknown => '#757575',
};

/// Añade la geocerca [shape] con su borde.
Future<void> addFence(
  MapLibreMapController map,
  GeofenceShape shape, {
  String color = '#1565c0',
}) async {
  final ring = [for (final p in drawableRing(shape)) toLatLng(p)];
  if (ring.length < 3) return;
  await map.addFill(
    FillOptions(geometry: [ring], fillColor: color, fillOpacity: 0.25),
  );
  await map.addLine(
    LineOptions(geometry: ring, lineColor: color, lineWidth: 2),
  );
}

/// Añade un punto con su círculo de precisión en metros.
Future<void> addPosition(
  MapLibreMapController map,
  GeoPoint point,
  double accuracyM, {
  String color = '#d32f2f',
}) async {
  if (accuracyM >= 1) {
    await map.addFill(
      FillOptions(
        geometry: [
          [
            for (final p in circleRing(point, accuracyM, segments: 32))
              toLatLng(p),
          ],
        ],
        fillColor: color,
        fillOpacity: 0.12,
      ),
    );
  }
  await map.addCircle(
    CircleOptions(
      geometry: toLatLng(point),
      circleRadius: 6,
      circleColor: color,
      circleStrokeColor: '#ffffff',
      circleStrokeWidth: 2,
    ),
  );
}

/// Borra todas las anotaciones.
Future<void> clearAll(MapLibreMapController map) async {
  await map.clearFills();
  await map.clearLines();
  await map.clearCircles();
}
