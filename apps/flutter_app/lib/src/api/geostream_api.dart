/// Cliente REST mínimo de GeoStream para la app de pruebas.
///
/// Solo usa operaciones ya implementadas en el servidor y permitidas a un
/// **token de dispositivo** (`docs/06_IDENTITY_AUTH.md` §8.2): crear sesión,
/// activar la compartición propia, ingesta HTTP y snapshot. La app nunca
/// maneja una API key: el token lo emite el backend del integrador.
library;

import 'dart:convert';
import 'dart:math';

import 'package:http/http.dart' as http;

/// Error de la API con el `Problem` (RFC 9457) del contrato.
///
/// Por diseño no guarda el cuerpo de la petición ni la credencial, para que
/// nunca acaben en un log o en la UI (`AGENTS.md`).
final class GeoStreamApiException implements Exception {
  /// Crea la excepción a partir de los campos del `Problem`.
  const GeoStreamApiException(
    this.status,
    this.code,
    this.requestId, [
    this.title,
  ]);

  /// Código HTTP.
  final int status;

  /// `ErrorCode` del contrato, o `NETWORK` si no hubo respuesta.
  final String code;

  /// `request_id` para soporte, si el servidor lo devolvió.
  final String? requestId;

  /// `title` del `Problem`, ya libre de datos sensibles por contrato.
  final String? title;

  @override
  String toString() =>
      'HTTP $status $code${title == null ? '' : ' — $title'}'
      '${requestId == null ? '' : ' ($requestId)'}';
}

/// Resultado por muestra de `POST /v1/ingest/locations`.
final class SampleResult {
  /// Crea un resultado.
  const SampleResult(this.sequence, this.status, this.errorCode);

  /// Secuencia de la muestra.
  final int sequence;

  /// `ACCEPTED`, `IGNORED_DUPLICATE`, `IGNORED_STALE` o `REJECTED`.
  final String status;

  /// `ErrorCode` si fue rechazada.
  final String? errorCode;
}

/// Posición de un miembro en el snapshot de la sesión.
final class SnapshotMember {
  /// Crea una entrada del snapshot.
  const SnapshotMember({
    required this.memberId,
    required this.externalSubjectId,
    required this.latitude,
    required this.longitude,
    required this.accuracyM,
    required this.ageMs,
  });

  /// `mem_…`.
  final String memberId;

  /// ID opaco del integrador.
  final String externalSubjectId;

  /// Latitud de la última muestra aceptada.
  final double latitude;

  /// Longitud de la última muestra aceptada.
  final double longitude;

  /// Precisión horizontal en metros.
  final double accuracyM;

  /// Edad de la muestra en milisegundos.
  final int ageMs;
}

/// Cliente REST autenticado con un token de dispositivo.
final class GeoStreamApi {
  /// Crea el cliente contra [baseUrl] (sin `/v1`) con [deviceToken].
  GeoStreamApi({
    required Uri baseUrl,
    required String deviceToken,
    http.Client? client,
    Random? random,
  }) : _base = baseUrl,
       _token = deviceToken,
       _client = client ?? http.Client(),
       _random = random ?? Random.secure();

  final Uri _base;
  final String _token;
  final http.Client _client;
  final Random _random;

  static const Duration _timeout = Duration(seconds: 15);

  Uri _uri(String path) =>
      _base.replace(path: '${_base.path}$path'.replaceAll(RegExp('/+'), '/'));

  Map<String, String> get _headers => {
    'authorization': 'Bearer $_token',
    'content-type': 'application/json',
    'accept': 'application/json',
  };

  String _idempotencyKey() {
    final bytes = List<int>.generate(16, (_) => _random.nextInt(256));
    return 'tester-${bytes.map((b) => b.toRadixString(16).padLeft(2, '0')).join()}';
  }

  Future<Map<String, dynamic>> _send(
    String method,
    String path, {
    Object? body,
    Map<String, String> extraHeaders = const {},
    bool auth = true,
  }) async {
    final request = http.Request(method, _uri(path))
      ..headers.addAll(auth ? _headers : const {'accept': 'application/json'})
      ..headers.addAll(extraHeaders);
    if (body != null) request.body = jsonEncode(body);
    final http.Response response;
    try {
      response = await http.Response.fromStream(
        await _client.send(request).timeout(_timeout),
      );
    } on Exception {
      throw const GeoStreamApiException(0, 'NETWORK', null, 'Sin conexión');
    }
    final decoded = response.body.isEmpty ? null : _tryDecode(response.body);
    if (response.statusCode >= 400) {
      final problem = decoded is Map<String, dynamic> ? decoded : const {};
      throw GeoStreamApiException(
        response.statusCode,
        (problem['code'] as String?) ?? 'HTTP_${response.statusCode}',
        problem['request_id'] as String?,
        problem['title'] as String?,
      );
    }
    return decoded is Map<String, dynamic> ? decoded : const {};
  }

  static Object? _tryDecode(String body) {
    try {
      return jsonDecode(body);
    } on FormatException {
      return null;
    }
  }

  /// `GET /health/ready`: devuelve `status` (`ok`, `degraded`…).
  Future<String> ready() async {
    final json = await _send('GET', '/health/ready', auth: false);
    return (json['status'] as String?) ?? 'unknown';
  }

  /// `POST /v1/sessions` en modo [mode]; devuelve el `session_id`.
  Future<String> createSession({String mode = 'ONE_TO_MANY'}) async {
    final json = await _send(
      'POST',
      '/v1/sessions',
      body: {'mode': mode},
      extraHeaders: {'idempotency-key': _idempotencyKey()},
    );
    return json['id'] as String;
  }

  /// `PUT /v1/sessions/{id}/members/me/sharing`. Solo el propio subject activa.
  Future<void> setSharing(String sessionId, {required bool sharing}) async {
    await _send(
      'PUT',
      '/v1/sessions/$sessionId/members/me/sharing',
      body: {'sharing': sharing},
    );
  }

  /// `POST /v1/ingest/locations` con hasta 50 muestras.
  Future<List<SampleResult>> ingest(
    String sessionId,
    List<Map<String, Object>> samples,
  ) async {
    final json = await _send(
      'POST',
      '/v1/ingest/locations',
      body: {'session_id': sessionId, 'samples': samples},
    );
    final results = (json['results'] as List<dynamic>? ?? const []);
    return [
      for (final r in results.cast<Map<String, dynamic>>())
        SampleResult(
          (r['sequence'] as num).toInt(),
          r['status'] as String,
          r['error_code'] as String?,
        ),
    ];
  }

  /// `GET /v1/sessions/{id}/snapshot`: última posición de cada miembro.
  Future<List<SnapshotMember>> snapshot(String sessionId) async {
    final json = await _send('GET', '/v1/sessions/$sessionId/snapshot');
    final items = (json['items'] as List<dynamic>? ?? const []);
    return [
      for (final item in items.cast<Map<String, dynamic>>())
        _snapshotMember(item),
    ];
  }

  static SnapshotMember _snapshotMember(Map<String, dynamic> item) {
    final sample = item['sample'] as Map<String, dynamic>;
    final pos = sample['position'] as Map<String, dynamic>;
    return SnapshotMember(
      memberId: item['member_id'] as String,
      externalSubjectId: item['external_subject_id'] as String,
      latitude: (pos['latitude'] as num).toDouble(),
      longitude: (pos['longitude'] as num).toDouble(),
      accuracyM: (pos['horizontal_accuracy_m'] as num).toDouble(),
      ageMs: (item['age_ms'] as num).toInt(),
    );
  }

  /// Cierra el cliente HTTP subyacente.
  void close() => _client.close();
}
