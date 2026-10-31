/// Conversión de una medición de plataforma a `LocationSample` del contrato.
///
/// Aplica las reglas de `docs/12_FLUTTER_SDK.md` §11 («Conversión desde la
/// plataforma») y los rangos de `Position` en el OpenAPI.
library;

/// Medición de plataforma independiente de `geolocator`, para poder probarla.
final class RawFix {
  /// Crea una medición; los campos opcionales pueden venir como NaN o negativos.
  const RawFix({
    required this.latitude,
    required this.longitude,
    required this.accuracyM,
    required this.timestamp,
    this.altitudeM,
    this.altitudeAccuracyM,
    this.speedMps,
    this.speedAccuracyMps,
    this.headingDeg,
    this.headingAccuracyDeg,
    this.isMock = false,
  });

  /// Latitud en grados.
  final double latitude;

  /// Longitud en grados.
  final double longitude;

  /// Precisión horizontal en metros.
  final double accuracyM;

  /// Instante de la medición según el reloj del dispositivo.
  final DateTime timestamp;

  /// Altitud en metros.
  final double? altitudeM;

  /// Precisión vertical en metros.
  final double? altitudeAccuracyM;

  /// Velocidad en m/s.
  final double? speedMps;

  /// Precisión de la velocidad en m/s.
  final double? speedAccuracyMps;

  /// Rumbo en grados.
  final double? headingDeg;

  /// Precisión del rumbo en grados.
  final double? headingAccuracyDeg;

  /// Whether la plataforma marcó la posición como simulada.
  final bool isMock;
}

bool _inRange(double? v, double minExclusive, double maxInclusive) =>
    v != null && v.isFinite && v > minExclusive && v <= maxInclusive;

/// Formatea [t] como RFC 3339 UTC con milisegundos y sufijo `Z`.
String rfc3339Millis(DateTime t) {
  final u = t.toUtc();
  String two(int v) => v.toString().padLeft(2, '0');
  final ms = u.millisecond.toString().padLeft(3, '0');
  return '${u.year.toString().padLeft(4, '0')}-${two(u.month)}-${two(u.day)}'
      'T${two(u.hour)}:${two(u.minute)}:${two(u.second)}.${ms}Z';
}

/// Convierte [fix] en `LocationSample` con [sequence]; `null` si se descarta.
Map<String, Object>? toLocationSample(RawFix fix, int sequence) {
  if (!fix.latitude.isFinite ||
      !fix.longitude.isFinite ||
      fix.latitude.abs() > 90 ||
      fix.longitude.abs() > 180) {
    return null;
  }
  if (!fix.accuracyM.isFinite || fix.accuracyM <= 0) return null;

  final position = <String, Object>{
    'latitude': fix.latitude,
    'longitude': fix.longitude,
    'horizontal_accuracy_m': fix.accuracyM,
  };
  final alt = fix.altitudeM;
  if (alt != null &&
      alt.isFinite &&
      alt >= -1000 &&
      alt <= 50000 &&
      _inRange(fix.altitudeAccuracyM, 0, 10000)) {
    position['altitude_m'] = alt;
    position['vertical_accuracy_m'] = fix.altitudeAccuracyM!;
  }
  final speed = fix.speedMps;
  if (speed != null && speed.isFinite && speed >= 0 && speed <= 300) {
    position['speed_mps'] = speed;
    if (_inRange(fix.speedAccuracyMps, 0, 300)) {
      position['speed_accuracy_mps'] = fix.speedAccuracyMps!;
    }
  }
  var heading = fix.headingDeg;
  if (heading != null && heading.isFinite && heading >= 0 && heading <= 360) {
    if (heading == 360) heading = 0;
    position['heading_deg'] = heading;
    if (_inRange(fix.headingAccuracyDeg, 0, 180)) {
      position['heading_accuracy_deg'] = fix.headingAccuracyDeg!;
    }
  }
  return {
    'sequence': sequence,
    'captured_at': rfc3339Millis(fix.timestamp),
    'position': position,
    'quality': {
      'kind': 'MEASURED',
      'source': 'UNSPECIFIED',
      'is_mock': fix.isMock,
    },
  };
}
