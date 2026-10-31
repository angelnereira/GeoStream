/// Estado de la app de pruebas: conexión, sesión, compartición y geocercas.
library;

import 'dart:async';

import 'package:flutter/foundation.dart';

import '../api/geostream_api.dart';
import '../geo/geodesy.dart';
import '../geo/geofence.dart';
import '../geo/geofence_evaluator.dart';
import '../storage/app_store.dart';
import 'sample_mapper.dart';

/// Fábrica del cliente REST, inyectable en pruebas.
typedef ApiFactory = GeoStreamApi Function(Uri baseUrl, String token);

/// Fuente de mediciones, inyectable en pruebas.
typedef FixSource = Stream<RawFix> Function();

/// Una línea del registro visible. Nunca contiene coordenadas ni credenciales.
final class LogLine {
  /// Crea una línea con la hora local actual.
  LogLine(this.message) : at = DateTime.now();

  /// Hora local.
  final DateTime at;

  /// Texto.
  final String message;
}

/// Última medición propia, para dibujarla en el mapa.
final class OwnFix {
  /// Crea la medición.
  const OwnFix(this.point, this.accuracyM, this.capturedAt);

  /// Posición.
  final GeoPoint point;

  /// Precisión horizontal en metros.
  final double accuracyM;

  /// Instante de captura.
  final DateTime capturedAt;
}

/// Controlador principal.
final class TrackingController extends ChangeNotifier {
  /// Crea el controlador.
  TrackingController({
    required this.store,
    required this.sequences,
    required this.apiFactory,
    required this.fixSource,
    this.flushInterval = const Duration(seconds: 2),
    this.snapshotInterval = const Duration(seconds: 5),
  }) : _fences = store.loadGeofences();

  /// Persistencia.
  final AppStore store;

  /// Secuencias por instalación.
  final SequenceAllocator sequences;

  /// Crea el cliente REST para una URL y un token.
  final ApiFactory apiFactory;

  /// Fuente de mediciones.
  final FixSource fixSource;

  /// Cada cuánto se envía la cola de muestras.
  final Duration flushInterval;

  /// Cada cuánto se lee el snapshot mientras se comparte.
  final Duration snapshotInterval;

  /// Máximo de muestras por petición de ingesta (`IngestRequest.samples`).
  static const int maxBatch = 50;

  GeoStreamApi? _api;
  String? _sessionId;
  bool _sharing = false;
  bool _busy = false;
  OwnFix? _lastFix;
  List<SnapshotMember> _members = const [];
  List<LocalGeofence> _fences;
  final GeofenceEvaluator _evaluator = GeofenceEvaluator();
  final List<Map<String, Object>> _queue = [];
  final List<LogLine> _log = [];
  StreamSubscription<RawFix>? _fixSub;
  Timer? _flushTimer;
  Timer? _snapshotTimer;
  int _accepted = 0;
  int _rejected = 0;
  int _dropped = 0;

  /// Muestras aceptadas por el servidor.
  int get accepted => _accepted;

  /// Muestras rechazadas por el servidor.
  int get rejected => _rejected;

  /// Muestras descartadas localmente o perdidas por fallo de red.
  int get dropped => _dropped;

  /// Whether hay un token cargado.
  bool get connected => _api != null;

  /// Sesión activa.
  String? get sessionId => _sessionId;

  /// Whether se está compartiendo.
  bool get sharing => _sharing;

  /// Whether hay una operación en curso.
  bool get busy => _busy;

  /// Última medición propia.
  OwnFix? get lastFix => _lastFix;

  /// Miembros del último snapshot.
  List<SnapshotMember> get members => _members;

  /// Geocercas locales.
  List<LocalGeofence> get fences => List.unmodifiable(_fences);

  /// Estado local de [fenceId].
  FenceState fenceState(String fenceId) => _evaluator.stateOf(fenceId);

  /// Registro, más reciente primero.
  List<LogLine> get log => List.unmodifiable(_log.reversed);

  void _note(String message) {
    _log.add(LogLine(message));
    if (_log.length > 200) _log.removeAt(0);
    notifyListeners();
  }

  Future<T?> _guard<T>(Future<T> Function() op) async {
    _busy = true;
    notifyListeners();
    try {
      return await op();
    } on GeoStreamApiException catch (e) {
      _note('Error: $e');
      return null;
    } finally {
      _busy = false;
      notifyListeners();
    }
  }

  /// Carga [token] contra [baseUrl] y comprueba `/health/ready`.
  Future<bool> connect(String baseUrl, String token) async {
    final uri = Uri.tryParse(baseUrl.trim());
    if (uri == null || !uri.hasScheme || token.trim().isEmpty) {
      _note('URL o token vacíos');
      return false;
    }
    await store.setBaseUrl(baseUrl.trim());
    _api?.close();
    _api = apiFactory(uri, token.trim());
    final status = await _guard(() => _api!.ready());
    if (status == null) return false;
    _note('Servidor: $status');
    return true;
  }

  /// Crea una sesión nueva y la deja activa.
  Future<void> createSession() async {
    final api = _api;
    if (api == null) return;
    final id = await _guard(() => api.createSession());
    if (id != null) useSession(id);
  }

  /// Usa una sesión existente (p. ej. creada por el backend del integrador).
  void useSession(String id) {
    _sessionId = id.trim();
    _members = const [];
    _evaluator.reset();
    _note('Sesión activa: $_sessionId');
  }

  /// Activa la compartición propia y empieza a publicar.
  Future<void> startSharing() async {
    final api = _api;
    final session = _sessionId;
    if (api == null || session == null || _sharing) return;
    final ok = await _guard(() async {
      await api.setSharing(session, sharing: true);
      return true;
    });
    if (ok != true) return;
    _sharing = true;
    _fixSub = fixSource().listen(
      _onFix,
      onError: (Object e) {
        _note('Ubicación no disponible: ${e.runtimeType}');
      },
    );
    _flushTimer = Timer.periodic(flushInterval, (_) => unawaited(flush()));
    _snapshotTimer = Timer.periodic(
      snapshotInterval,
      (_) => unawaited(refreshSnapshot()),
    );
    _note('Compartiendo');
  }

  /// Detiene la compartición. Las muestras pendientes se descartan, nunca se
  /// envían después de que el usuario apague la compartición.
  Future<void> stopSharing() async {
    final api = _api;
    final session = _sessionId;
    await _fixSub?.cancel();
    _fixSub = null;
    _flushTimer?.cancel();
    _snapshotTimer?.cancel();
    _queue.clear();
    _evaluator.reset();
    final wasSharing = _sharing;
    _sharing = false;
    notifyListeners();
    if (wasSharing && api != null && session != null) {
      await _guard(() => api.setSharing(session, sharing: false));
      _note('Compartición detenida');
    }
  }

  Future<void> _onFix(RawFix fix) async {
    if (!_sharing) return;
    final seq = await sequences.next();
    final sample = toLocationSample(fix, seq);
    if (sample == null) {
      _dropped++;
      notifyListeners();
      return;
    }
    final point = GeoPoint(fix.latitude, fix.longitude);
    _lastFix = OwnFix(point, fix.accuracyM, fix.timestamp);
    _queue.add(sample);
    if (_queue.length >= maxBatch) unawaited(flush());
    notifyListeners();
  }

  /// Envía las muestras en cola; las aceptadas alimentan la vista previa local.
  Future<void> flush() async {
    final api = _api;
    final session = _sessionId;
    if (api == null || session == null || _queue.isEmpty) return;
    final batch = _queue.take(maxBatch).toList();
    _queue.removeRange(0, batch.length);
    try {
      final results = await api.ingest(session, batch);
      final bySeq = {for (final s in batch) s['sequence']! as int: s};
      for (final r in results) {
        if (r.status == 'ACCEPTED') {
          _accepted++;
          final s = bySeq[r.sequence];
          if (s != null) _evaluateFences(s);
        } else if (r.status == 'REJECTED') {
          _rejected++;
          _note('Muestra ${r.sequence} rechazada: ${r.errorCode}');
        }
      }
      notifyListeners();
    } on GeoStreamApiException catch (e) {
      // Se pierden: la app de pruebas no reintenta, para que los fallos se vean.
      _dropped += batch.length;
      _note('Ingesta fallida: $e');
    }
  }

  void _evaluateFences(Map<String, Object> sample) {
    final pos = sample['position']! as Map<String, Object>;
    final events = _evaluator.evaluate(
      position: GeoPoint(
        pos['latitude']! as double,
        pos['longitude']! as double,
      ),
      accuracyM: pos['horizontal_accuracy_m']! as double,
      capturedAt: DateTime.parse(sample['captured_at']! as String),
      fences: _fences,
    );
    for (final e in events) {
      final name = _fences.firstWhere((f) => f.id == e.geofenceId).name;
      _note(
        e.transition == FenceTransition.entered
            ? 'Entró en «$name» (vista previa local)'
            : 'Salió de «$name» (vista previa local)',
      );
    }
  }

  /// Lee el snapshot de la sesión.
  Future<void> refreshSnapshot() async {
    final api = _api;
    final session = _sessionId;
    if (api == null || session == null) return;
    try {
      _members = await api.snapshot(session);
      notifyListeners();
    } on GeoStreamApiException catch (e) {
      if (e.status != 403) _note('Snapshot: $e');
    }
  }

  /// Guarda o reemplaza [fence].
  Future<void> upsertFence(LocalGeofence fence) async {
    _fences = [
      for (final f in _fences)
        if (f.id != fence.id) f,
      fence,
    ];
    await store.saveGeofences(_fences);
    notifyListeners();
  }

  /// Borra la geocerca [id].
  Future<void> deleteFence(String id) async {
    _fences = [
      for (final f in _fences)
        if (f.id != id) f,
    ];
    await store.saveGeofences(_fences);
    notifyListeners();
  }

  @override
  void dispose() {
    unawaited(_fixSub?.cancel());
    _flushTimer?.cancel();
    _snapshotTimer?.cancel();
    _api?.close();
    super.dispose();
  }
}
