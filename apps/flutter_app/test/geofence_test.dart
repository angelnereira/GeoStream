import 'package:flutter_test/flutter_test.dart';
import 'package:geostream_tester/src/geo/geodesy.dart';
import 'package:geostream_tester/src/geo/geofence.dart';

void main() {
  const c = GeoPoint(8.98, -79.52);

  group('círculo (14 §3)', () {
    test('radio en el rango es válido', () {
      expect(validateShape(const CircleShape(c, 25)), isEmpty);
      expect(validateShape(const CircleShape(c, 50000)), isEmpty);
    });

    test('radio fuera de rango → out_of_range', () {
      for (final r in [24.9, 50000.1, double.nan]) {
        final v = validateShape(CircleShape(c, r));
        expect(v.single.field, 'shape.radius_m');
        expect(v.single.reason, 'out_of_range');
      }
    });

    test('centro inválido', () {
      final v = validateShape(const CircleShape(GeoPoint(91, 0), 100));
      expect(v.single.field, 'shape.center');
    });
  });

  group('polígono (14 §3)', () {
    final tri = PolygonShape.fromVertices(const [
      GeoPoint(0, 0),
      GeoPoint(0, 0.01),
      GeoPoint(0.01, 0),
    ]);

    test('fromVertices cierra el anillo', () {
      expect(tri.ring.length, 4);
      expect(tri.ring.first, tri.ring.last);
      expect(validateShape(tri), isEmpty);
    });

    test('menos de 4 puntos → out_of_range', () {
      final v = validateShape(
        const PolygonShape([GeoPoint(0, 0), GeoPoint(0, 1), GeoPoint(0, 0)]),
      );
      expect(v.single.reason, 'out_of_range');
    });

    test('más de 101 puntos → out_of_range', () {
      final ring = [
        for (var i = 0; i < 101; i++) destination(c, i * 360 / 101, 500),
      ];
      expect(
        validateShape(PolygonShape.fromVertices(ring)).single.reason,
        'out_of_range',
      );
      expect(
        validateShape(PolygonShape.fromVertices(ring.sublist(0, 100))),
        isEmpty,
      );
    });

    test('anillo abierto → invalid_format', () {
      final v = validateShape(
        const PolygonShape([
          GeoPoint(0, 0),
          GeoPoint(0, 0.01),
          GeoPoint(0.01, 0.01),
          GeoPoint(0.01, 0),
        ]),
      );
      expect(v.map((e) => e.reason), contains('invalid_format'));
    });

    test('autointersección → invalid_format', () {
      final v = validateShape(
        PolygonShape.fromVertices(const [
          GeoPoint(0, 0),
          GeoPoint(0.01, 0.01),
          GeoPoint(0, 0.01),
          GeoPoint(0.01, 0),
        ]),
      );
      expect(v.map((e) => e.message), contains(contains('cruzarse')));
    });

    test('caja envolvente > 100 km → out_of_range', () {
      final v = validateShape(
        PolygonShape.fromVertices(const [
          GeoPoint(0, 0),
          GeoPoint(0, 1),
          GeoPoint(1, 0),
        ]),
      );
      expect(v.map((e) => e.message), contains(contains('100 km')));
    });

    test('área cero → out_of_range', () {
      final v = validateShape(
        PolygonShape.fromVertices(const [
          GeoPoint(0, 0),
          GeoPoint(0, 0.005),
          GeoPoint(0, 0.01),
        ]),
      );
      expect(v.map((e) => e.message), contains(contains('área')));
    });
  });

  test('nombre vacío o > 100 caracteres', () {
    LocalGeofence named(String n) =>
        LocalGeofence(id: 'x', name: n, shape: const CircleShape(c, 100));
    expect(named('Casa').validate(), isEmpty);
    expect(named('  ').validate().single.field, 'name');
    expect(named('a' * 101).validate().single.field, 'name');
  });

  test('toCreateRequest tiene la forma de CreateGeofenceRequest', () {
    const f = LocalGeofence(
      id: 'local_1',
      name: 'Oficina',
      sessionId: 'ses_0123',
      shape: CircleShape(c, 120),
    );
    expect(f.toCreateRequest(), {
      'name': 'Oficina',
      'session_id': 'ses_0123',
      'shape': {
        'type': 'circle',
        'center': {'latitude': 8.98, 'longitude': -79.52},
        'radius_m': 120.0,
      },
    });
    const noSession = LocalGeofence(
      id: 'l',
      name: 'n',
      shape: CircleShape(c, 30),
    );
    expect(noSession.toCreateRequest().containsKey('session_id'), isFalse);
  });

  test('ida y vuelta JSON local', () {
    final f = LocalGeofence(
      id: 'local_2',
      name: 'Zona',
      shape: PolygonShape.fromVertices(const [
        GeoPoint(0, 0),
        GeoPoint(0, 0.01),
        GeoPoint(0.01, 0),
      ]),
    );
    final back = LocalGeofence.fromJson(Map<String, dynamic>.from(f.toJson()));
    expect(back.toJson(), f.toJson());
  });
}
