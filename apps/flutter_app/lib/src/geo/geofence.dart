/// Geocercas del editor: modelo, validación y JSON del contrato.
///
/// La forma y las reglas son las de `CreateGeofenceRequest` en
/// `contracts/openapi/geostream.v1.yaml` y `docs/14_GEOFENCING.md` §3, para que
/// una geocerca dibujada aquí se pueda enviar tal cual cuando exista
/// `POST /v1/geofences` (fase F11).
library;

import 'geodesy.dart';

/// Radio mínimo de un círculo en metros (`14` §3).
const double minRadiusM = 25;

/// Radio máximo de un círculo en metros (`14` §3).
const double maxRadiusM = 50000;

/// Puntos mínimos del anillo cerrado de un polígono (`14` §3).
const int minRingPoints = 4;

/// Puntos máximos del anillo cerrado de un polígono (`14` §3).
const int maxRingPoints = 101;

/// Lado máximo de la caja envolvente de un polígono en metros (`14` §3).
const double maxPolygonSpanM = 100000;

/// Longitud máxima del nombre (`14` §2).
const int maxNameLength = 100;

/// Forma de una geocerca: círculo o polígono.
sealed class GeofenceShape {
  const GeofenceShape();

  /// Objeto `GeofenceShape` del contrato.
  Map<String, Object> toJson();

  /// Lee un `GeofenceShape`; lanza [FormatException] si el tipo no se conoce.
  static GeofenceShape fromJson(Map<String, dynamic> json) {
    switch (json['type']) {
      case 'circle':
        final r = json['radius_m'];
        if (r is! num) throw const FormatException('radius_m ausente');
        return CircleShape(
          GeoPoint.fromJson(json['center'] as Map<String, dynamic>),
          r.toDouble(),
        );
      case 'polygon':
        final ring = (json['ring'] as List<dynamic>)
            .map((p) => GeoPoint.fromJson(p as Map<String, dynamic>))
            .toList();
        return PolygonShape(ring);
      default:
        throw const FormatException('tipo de forma desconocido');
    }
  }
}

/// Círculo de [radiusM] metros alrededor de [center].
final class CircleShape extends GeofenceShape {
  /// Crea un círculo; validar con [validateShape].
  const CircleShape(this.center, this.radiusM);

  /// Centro del círculo.
  final GeoPoint center;

  /// Radio en metros, 25..=50000.
  final double radiusM;

  @override
  Map<String, Object> toJson() => {
    'type': 'circle',
    'center': center.toJson(),
    'radius_m': radiusM,
  };
}

/// Polígono definido por un anillo exterior cerrado (primer punto = último).
final class PolygonShape extends GeofenceShape {
  /// Crea un polígono; validar con [validateShape].
  const PolygonShape(this.ring);

  /// Anillo exterior cerrado.
  final List<GeoPoint> ring;

  /// Cierra una lista de vértices abierta repitiendo el primero al final.
  factory PolygonShape.fromVertices(List<GeoPoint> vertices) {
    if (vertices.isEmpty || vertices.first == vertices.last) {
      return PolygonShape(List.unmodifiable(vertices));
    }
    return PolygonShape(List.unmodifiable([...vertices, vertices.first]));
  }

  @override
  Map<String, Object> toJson() => {
    'type': 'polygon',
    'ring': [for (final p in ring) p.toJson()],
  };
}

/// Una regla de `14` §3 que la geometría no cumple.
final class GeofenceViolation {
  /// Crea una violación con el [field] del contrato y su [reason] (`09`).
  const GeofenceViolation(this.field, this.reason, this.message);

  /// Ruta del campo, p. ej. `shape.radius_m`.
  final String field;

  /// Razón de validación de `09` (`out_of_range`, `invalid_format`, …).
  final String reason;

  /// Explicación legible para la UI. Nunca contiene coordenadas.
  final String message;

  @override
  String toString() => '$field: $message';
}

/// Valida [shape] con las reglas de `14` §3; lista vacía si es válida.
List<GeofenceViolation> validateShape(GeofenceShape shape) {
  return switch (shape) {
    CircleShape(:final center, :final radiusM) => [
      if (!center.isValid)
        const GeofenceViolation(
          'shape.center',
          'out_of_range',
          'Centro con latitud o longitud fuera de rango.',
        ),
      if (!radiusM.isFinite || radiusM < minRadiusM || radiusM > maxRadiusM)
        const GeofenceViolation(
          'shape.radius_m',
          'out_of_range',
          'El radio debe estar entre 25 y 50000 m.',
        ),
    ],
    PolygonShape(:final ring) => _validateRing(ring),
  };
}

List<GeofenceViolation> _validateRing(List<GeoPoint> ring) {
  if (ring.length < minRingPoints || ring.length > maxRingPoints) {
    return const [
      GeofenceViolation(
        'shape.ring',
        'out_of_range',
        'El polígono necesita de 3 a 100 vértices.',
      ),
    ];
  }
  final out = <GeofenceViolation>[];
  if (ring.any((p) => !p.isValid)) {
    out.add(
      const GeofenceViolation(
        'shape.ring',
        'out_of_range',
        'Algún vértice tiene latitud o longitud fuera de rango.',
      ),
    );
    return out;
  }
  if (ring.first != ring.last) {
    out.add(
      const GeofenceViolation(
        'shape.ring',
        'invalid_format',
        'El anillo debe estar cerrado (primer punto = último).',
      ),
    );
  }
  for (var i = 0; i + 1 < ring.length; i++) {
    if ((ring[i + 1].longitude - ring[i].longitude).abs() > 180) {
      out.add(
        const GeofenceViolation(
          'shape.ring',
          'out_of_range',
          'El polígono no puede cruzar el antimeridiano.',
        ),
      );
      break;
    }
  }
  if (ringSelfIntersects(ring)) {
    out.add(
      const GeofenceViolation(
        'shape.ring',
        'invalid_format',
        'El polígono no puede cruzarse a sí mismo.',
      ),
    );
  }
  if (ringAreaM2(ring) <= 0) {
    out.add(
      const GeofenceViolation(
        'shape.ring',
        'out_of_range',
        'El polígono debe tener área.',
      ),
    );
  }
  final lats = ring.map((p) => p.latitude);
  final lons = ring.map((p) => p.longitude);
  final minLat = lats.reduce((a, b) => a < b ? a : b);
  final maxLat = lats.reduce((a, b) => a > b ? a : b);
  final minLon = lons.reduce((a, b) => a < b ? a : b);
  final maxLon = lons.reduce((a, b) => a > b ? a : b);
  final midLat = (minLat + maxLat) / 2;
  final heightM = haversineM(
    GeoPoint(minLat, minLon),
    GeoPoint(maxLat, minLon),
  );
  final widthM = haversineM(GeoPoint(midLat, minLon), GeoPoint(midLat, maxLon));
  if (heightM > maxPolygonSpanM || widthM > maxPolygonSpanM) {
    out.add(
      const GeofenceViolation(
        'shape.ring',
        'out_of_range',
        'La caja envolvente supera 100 km por lado.',
      ),
    );
  }
  return out;
}

/// Geocerca guardada en el dispositivo.
final class LocalGeofence {
  /// Crea una geocerca local con [id] estable generado por el editor.
  const LocalGeofence({
    required this.id,
    required this.name,
    required this.shape,
    this.sessionId,
  });

  /// Identificador local (no es un `geo_…` del servidor).
  final String id;

  /// Nombre de 1 a 100 caracteres.
  final String name;

  /// Forma de la geocerca.
  final GeofenceShape shape;

  /// Sesión a la que se asociará al enviarla, si la hay.
  final String? sessionId;

  /// Valida nombre y forma; lista vacía si es válida.
  List<GeofenceViolation> validate() => [
    if (name.trim().isEmpty || name.length > maxNameLength)
      const GeofenceViolation(
        'name',
        'out_of_range',
        'El nombre debe tener de 1 a 100 caracteres.',
      ),
    ...validateShape(shape),
  ];

  /// Cuerpo `CreateGeofenceRequest` del contrato, listo para F11.
  Map<String, Object> toCreateRequest() => {
    'name': name,
    'session_id': ?sessionId,
    'shape': shape.toJson(),
  };

  /// Representación persistida en el dispositivo.
  Map<String, Object?> toJson() => {
    'id': id,
    'name': name,
    'session_id': sessionId,
    'shape': shape.toJson(),
  };

  /// Lee la representación persistida.
  static LocalGeofence fromJson(Map<String, dynamic> json) => LocalGeofence(
    id: json['id'] as String,
    name: json['name'] as String,
    sessionId: json['session_id'] as String?,
    shape: GeofenceShape.fromJson(json['shape'] as Map<String, dynamic>),
  );
}
