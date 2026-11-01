import 'dart:convert';

import 'package:http/http.dart' as http;

import 'models.dart';

/// La API rechazó la API key (401 o 403): no existe, está revocada o no tiene el scope.
class ApiKeyRejected implements Exception {
  /// Crea la excepción.
  const ApiKeyRejected(this.statusCode);

  /// Código HTTP recibido.
  final int statusCode;

  @override
  String toString() => 'ApiKeyRejected($statusCode)';
}

/// La API no respondió o respondió algo que no cumple el contrato.
class ApiUnavailable implements Exception {
  /// Crea la excepción.
  const ApiUnavailable();

  @override
  String toString() => 'ApiUnavailable';
}

/// Cliente de solo lectura de la API a través del proxy del portal (`/api/…`, ADR-0035).
///
/// La API key vive solo en esta instancia (memoria): nunca se guarda ni se escribe en logs.
class ApiClient {
  /// Crea el cliente. [base] termina en `/` (por ejemplo `https://portal/api/`).
  ApiClient({
    required this.client,
    required this.base,
    required this._apiKey,
    this.timeout = const Duration(seconds: 8),
  });

  /// Cliente HTTP.
  final http.Client client;

  /// Base del proxy.
  final Uri base;

  /// Tiempo máximo por petición.
  final Duration timeout;

  final String _apiKey;

  /// Lista las sesiones activas de la aplicación (`GET /v1/sessions`).
  Future<List<SessionSummary>> listSessions() async {
    final body = await _get('v1/sessions', {'status': 'ACTIVE', 'limit': '50'});
    final data = body['data'];
    if (data is! List<Object?>) throw const ApiUnavailable();
    return [for (final item in data) ?SessionSummary.fromJson(item)];
  }

  /// Últimas posiciones de los miembros que comparten (`GET /v1/sessions/{id}/snapshot`).
  Future<List<MemberPosition>> snapshot(String sessionId) async {
    final body = await _get('v1/sessions/$sessionId/snapshot');
    final items = body['items'];
    if (items is! List<Object?>) throw const ApiUnavailable();
    return [for (final item in items) ?MemberPosition.fromJson(item)];
  }

  Future<Map<String, Object?>> _get(
    String path, [
    Map<String, String>? query,
  ]) async {
    final uri = base.resolve(path).replace(queryParameters: query);
    final http.Response response;
    try {
      response = await client
          .get(uri, headers: {'Authorization': 'Bearer $_apiKey'})
          .timeout(timeout);
    } on Object {
      throw const ApiUnavailable();
    }
    if (response.statusCode == 401 || response.statusCode == 403) {
      throw ApiKeyRejected(response.statusCode);
    }
    if (response.statusCode != 200) throw const ApiUnavailable();
    try {
      final json = jsonDecode(response.body);
      if (json is Map<String, Object?>) return json;
    } on FormatException {
      // Un cuerpo que no es JSON es lo mismo que no tener respuesta.
    }
    throw const ApiUnavailable();
  }
}
