import 'package:flutter_test/flutter_test.dart';
import 'package:geostream_tester/src/geo/geodesy.dart';
import 'package:geostream_tester/src/geo/geofence.dart';
import 'package:geostream_tester/src/geo/geofence_evaluator.dart';

void main() {
  const center = GeoPoint(8.98, -79.52);
  const fence = LocalGeofence(
    id: 'f',
    name: 'F',
    shape: CircleShape(center, 100),
  );
  final t0 = DateTime.utc(2026, 10, 31);
  final inside = destination(center, 0, 50); // d = -50
  final outside = destination(center, 0, 150); // d = +50
  final ambiguous = destination(center, 0, 110); // d = +10 < buffer 20

  List<LocalFenceEvent> feed(
    GeofenceEvaluator e,
    GeoPoint p, {
    double acc = 10,
    int i = 0,
  }) => e.evaluate(
    position: p,
    accuracyM: acc,
    capturedAt: t0.add(Duration(seconds: i)),
    fences: const [fence],
  );

  test('UNKNOWN → INSIDE tras 2 IN emite ENTERED con la 2.ª muestra', () {
    final e = GeofenceEvaluator();
    expect(feed(e, inside, i: 1), isEmpty);
    final ev = feed(e, inside, i: 2);
    expect(ev.single.transition, FenceTransition.entered);
    expect(ev.single.occurredAt, t0.add(const Duration(seconds: 2)));
    expect(e.stateOf('f'), FenceState.inside);
  });

  test('UNKNOWN → OUTSIDE no emite', () {
    final e = GeofenceEvaluator();
    feed(e, outside);
    expect(feed(e, outside), isEmpty);
    expect(e.stateOf('f'), FenceState.outside);
  });

  test(
    'INSIDE → OUTSIDE emite EXITED; ambiguas y baja precisión no cuentan',
    () {
      final e = GeofenceEvaluator();
      feed(e, inside);
      feed(e, inside);
      expect(feed(e, outside), isEmpty);
      expect(feed(e, ambiguous), isEmpty); // no altera el contador
      expect(feed(e, outside, acc: 150), isEmpty); // ignorada
      final ev = feed(e, outside);
      expect(ev.single.transition, FenceTransition.exited);
    },
  );

  test('una IN aislada reinicia el contador opuesto', () {
    final e = GeofenceEvaluator();
    feed(e, inside);
    feed(e, inside);
    feed(e, outside);
    feed(e, inside);
    expect(feed(e, outside), isEmpty);
    expect(e.stateOf('f'), FenceState.inside);
  });

  test('reset borra el estado sin emitir', () {
    final e = GeofenceEvaluator();
    feed(e, inside);
    feed(e, inside);
    e.reset();
    expect(e.stateOf('f'), FenceState.unknown);
  });

  test('polígono: dentro/fuera con distancia firmada', () {
    final poly = PolygonShape.fromVertices(const [
      GeoPoint(0, 0),
      GeoPoint(0, 0.01),
      GeoPoint(0.01, 0.01),
      GeoPoint(0.01, 0),
    ]);
    expect(signedDistanceM(const GeoPoint(0.005, 0.005), poly), lessThan(0));
    expect(signedDistanceM(const GeoPoint(0.02, 0.005), poly), greaterThan(20));
  });
}
