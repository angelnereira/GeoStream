import 'package:flutter_test/flutter_test.dart';
import 'package:geostream_tester/src/geo/geodesy.dart';

void main() {
  test('haversine de 1 grado de latitud ≈ 111.2 km', () {
    final d = haversineM(const GeoPoint(0, 0), const GeoPoint(1, 0));
    expect(d, closeTo(111195, 5));
  });

  test('destination y haversine son coherentes', () {
    const o = GeoPoint(8.98, -79.52);
    final p = destination(o, 45, 1000);
    expect(haversineM(o, p), closeTo(1000, 0.5));
  });

  test('circleRing cierra el anillo y respeta el radio', () {
    const c = GeoPoint(8.98, -79.52);
    final ring = circleRing(c, 200, segments: 16);
    expect(ring.length, 17);
    expect(ring.first, ring.last);
    for (final p in ring) {
      expect(haversineM(c, p), closeTo(200, 0.5));
    }
  });

  final square = [
    const GeoPoint(0, 0),
    const GeoPoint(0, 0.01),
    const GeoPoint(0.01, 0.01),
    const GeoPoint(0.01, 0),
    const GeoPoint(0, 0),
  ];

  test('distancia firmada: negativa dentro, positiva fuera', () {
    expect(
      signedDistanceToRingM(const GeoPoint(0.005, 0.005), square),
      lessThan(0),
    );
    final outside = signedDistanceToRingM(const GeoPoint(0.005, 0.011), square);
    expect(outside, closeTo(111.2, 1));
  });

  test('área del cuadrado ≈ 1.236 km²', () {
    expect(ringAreaM2(square), closeTo(1112 * 1112, 5000));
  });

  test('detecta autointersección (pajarita)', () {
    final bowtie = [
      const GeoPoint(0, 0),
      const GeoPoint(0.01, 0.01),
      const GeoPoint(0, 0.01),
      const GeoPoint(0.01, 0),
      const GeoPoint(0, 0),
    ];
    expect(ringSelfIntersects(bowtie), isTrue);
    expect(ringSelfIntersects(square), isFalse);
  });
}
