/// Evaluación local de geocercas con la histéresis de `docs/14_GEOFENCING.md` §4.
///
/// Es una **vista previa** para probar en campo: el servidor es la fuente de
/// verdad y descarta cualquier evento de geocerca emitido por un cliente
/// (`14` §5.1). Nada de aquí se envía al servidor.
library;

import 'geodesy.dart';
import 'geofence.dart';

/// Parámetros de histéresis, con los defaults de `docs/17_CONFIGURATION.md`.
final class HysteresisParams {
  /// Crea parámetros; los valores por defecto son los de la política.
  const HysteresisParams({
    this.confirmSamples = 2,
    this.exitBufferM = 20,
    this.maxAccuracyM = 100,
  });

  /// `geofence_confirm_samples`, 1..=10.
  final int confirmSamples;

  /// `geofence_exit_buffer_m`, 0..=500.
  final double exitBufferM;

  /// `geofence_max_accuracy_m`, 10..=1000.
  final double maxAccuracyM;
}

/// Estado de un miembro respecto a una geocerca.
enum FenceState {
  /// Aún no hay suficientes clasificaciones.
  unknown,

  /// Dentro, confirmado.
  inside,

  /// Fuera, confirmado.
  outside,
}

/// Transición emitida al confirmar un cambio de estado.
enum FenceTransition {
  /// Entró en la geocerca.
  entered,

  /// Salió de la geocerca.
  exited,
}

/// Una transición local confirmada.
final class LocalFenceEvent {
  /// Crea un evento para [geofenceId] confirmado por la muestra de [occurredAt].
  const LocalFenceEvent(this.geofenceId, this.transition, this.occurredAt);

  /// Geocerca afectada.
  final String geofenceId;

  /// Entrada o salida.
  final FenceTransition transition;

  /// `captured_at` de la muestra que confirmó la transición.
  final DateTime occurredAt;
}

enum _Class { inside, outside, ambiguous }

final class _Tracker {
  FenceState state = FenceState.unknown;
  _Class? pending;
  int count = 0;
}

/// Distancia firmada en metros de [p] al borde de [shape]: negativa dentro.
double signedDistanceM(GeoPoint p, GeofenceShape shape) => switch (shape) {
  CircleShape(:final center, :final radiusM) => haversineM(p, center) - radiusM,
  PolygonShape(:final ring) => signedDistanceToRingM(p, ring),
};

/// Evaluador con estado por geocerca para un solo dispositivo.
final class GeofenceEvaluator {
  /// Crea un evaluador con [params].
  GeofenceEvaluator({this.params = const HysteresisParams()});

  /// Parámetros de histéresis en uso.
  final HysteresisParams params;

  final Map<String, _Tracker> _trackers = {};

  /// Estado actual de [geofenceId].
  FenceState stateOf(String geofenceId) =>
      _trackers[geofenceId]?.state ?? FenceState.unknown;

  /// Borra todo el estado sin emitir eventos (`14` §4 paso 6).
  void reset() => _trackers.clear();

  /// Evalúa una muestra aceptada contra [fences]; devuelve las transiciones.
  List<LocalFenceEvent> evaluate({
    required GeoPoint position,
    required double accuracyM,
    required DateTime capturedAt,
    required Iterable<LocalGeofence> fences,
  }) {
    if (!(accuracyM <= params.maxAccuracyM)) return const [];
    final live = <String>{};
    final events = <LocalFenceEvent>[];
    for (final fence in fences) {
      live.add(fence.id);
      final d = signedDistanceM(position, fence.shape);
      final cls = d <= 0
          ? _Class.inside
          : (d >= params.exitBufferM ? _Class.outside : _Class.ambiguous);
      if (cls == _Class.ambiguous) continue;
      final t = _trackers.putIfAbsent(fence.id, _Tracker.new);
      final agrees =
          (cls == _Class.inside && t.state == FenceState.inside) ||
          (cls == _Class.outside && t.state == FenceState.outside);
      if (agrees) {
        t
          ..pending = null
          ..count = 0;
        continue;
      }
      t.count = t.pending == cls ? t.count + 1 : 1;
      t.pending = cls;
      if (t.count < params.confirmSamples) continue;
      final previous = t.state;
      t
        ..state = cls == _Class.inside ? FenceState.inside : FenceState.outside
        ..pending = null
        ..count = 0;
      if (t.state == FenceState.inside) {
        events.add(
          LocalFenceEvent(fence.id, FenceTransition.entered, capturedAt),
        );
      } else if (previous == FenceState.inside) {
        events.add(
          LocalFenceEvent(fence.id, FenceTransition.exited, capturedAt),
        );
      }
    }
    _trackers.removeWhere((id, _) => !live.contains(id));
    return events;
  }
}
