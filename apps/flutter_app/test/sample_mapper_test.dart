import 'package:flutter_test/flutter_test.dart';
import 'package:geostream_tester/src/location/sample_mapper.dart';

void main() {
  final t = DateTime.utc(2026, 10, 31, 12, 0, 5, 7);

  RawFix fix({
    double acc = 4,
    double? alt = 10,
    double? altAcc = 3,
    double? speed = 1.5,
    double? heading = 90,
  }) => RawFix(
    latitude: 8.98,
    longitude: -79.52,
    accuracyM: acc,
    timestamp: t,
    altitudeM: alt,
    altitudeAccuracyM: altAcc,
    speedMps: speed,
    speedAccuracyMps: 0.5,
    headingDeg: heading,
    headingAccuracyDeg: 10,
  );

  test('captured_at en RFC 3339 UTC con milisegundos', () {
    expect(rfc3339Millis(t), '2026-10-31T12:00:05.007Z');
  });

  test('muestra completa', () {
    final s = toLocationSample(fix(), 7)!;
    expect(s['sequence'], 7);
    expect(s['captured_at'], '2026-10-31T12:00:05.007Z');
    expect(s['position'], {
      'latitude': 8.98,
      'longitude': -79.52,
      'horizontal_accuracy_m': 4.0,
      'altitude_m': 10.0,
      'vertical_accuracy_m': 3.0,
      'speed_mps': 1.5,
      'speed_accuracy_mps': 0.5,
      'heading_deg': 90.0,
      'heading_accuracy_deg': 10.0,
    });
    expect((s['quality']! as Map)['kind'], 'MEASURED');
  });

  test('precisión ≤ 0 o NaN → se descarta', () {
    expect(toLocationSample(fix(acc: 0), 1), isNull);
    expect(toLocationSample(fix(acc: double.nan), 1), isNull);
  });

  test('velocidad/rumbo negativos o NaN → ausentes; rumbo 360 → 0', () {
    final p1 =
        toLocationSample(fix(speed: -1, heading: -1), 1)!['position']! as Map;
    expect(p1.containsKey('speed_mps'), isFalse);
    expect(p1.containsKey('heading_deg'), isFalse);
    final p2 =
        toLocationSample(fix(speed: double.nan, heading: 360), 1)!['position']!
            as Map;
    expect(p2.containsKey('speed_mps'), isFalse);
    expect(p2['heading_deg'], 0.0);
  });

  test('altitud sin precisión vertical → ausente', () {
    final p = toLocationSample(fix(altAcc: 0), 1)!['position']! as Map;
    expect(p.containsKey('altitude_m'), isFalse);
    expect(p.containsKey('vertical_accuracy_m'), isFalse);
  });
}
