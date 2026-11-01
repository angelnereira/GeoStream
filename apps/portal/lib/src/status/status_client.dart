import 'dart:convert';

import 'package:http/http.dart' as http;

import 'readiness.dart';

/// Consulta el estado de la API a través del proxy del mismo origen (`/status/ready`).
///
/// El proxy evita CORS y no expone ninguna credencial (ADR-0034 §3).
class StatusClient {
  /// Crea el cliente; `clock` permite fijar la hora en pruebas.
  StatusClient({
    required this.client,
    required this.endpoint,
    DateTime Function()? clock,
    this.timeout = const Duration(seconds: 5),
  }) : _clock = clock ?? (() => DateTime.now().toUtc());

  /// Cliente HTTP.
  final http.Client client;

  /// Dirección del proxy de estado.
  final Uri endpoint;

  final DateTime Function() _clock;

  /// Tiempo máximo de espera de una consulta.
  final Duration timeout;

  /// Devuelve el estado actual o [ReadinessUnknown] ante cualquier fallo.
  ///
  /// 200 y 503 son respuestas válidas del contrato; el resto, o un cuerpo que no cumple el
  /// esquema, es «sin datos».
  Future<ReadinessSnapshot> fetch() async {
    try {
      final response = await client.get(endpoint).timeout(timeout);
      if (response.statusCode != 200 && response.statusCode != 503) {
        return ReadinessUnknown(_clock());
      }
      return parse(response.body, _clock());
    } on Object {
      // Cualquier fallo de red, de tiempo o de formato es «sin datos», nunca «disponible».
      return ReadinessUnknown(_clock());
    }
  }

  /// Interpreta el cuerpo de `/health/ready` según el esquema `ReadinessStatus`.
  static ReadinessSnapshot parse(String body, DateTime checkedAt) {
    final Object? json;
    try {
      json = jsonDecode(body);
    } on FormatException {
      return ReadinessUnknown(checkedAt);
    }
    if (json is! Map<String, Object?>) return ReadinessUnknown(checkedAt);
    final status = json['status'];
    final rawChecks = json['checks'];
    if ((status != 'ready' && status != 'not_ready') ||
        rawChecks is! Map<String, Object?>) {
      return ReadinessUnknown(checkedAt);
    }
    final checks = <String, CheckState>{};
    for (final entry in rawChecks.entries) {
      final state = CheckState.parse(entry.value);
      if (state == null) return ReadinessUnknown(checkedAt);
      checks[entry.key] = state;
    }
    return ReadinessKnown(
      checkedAt: checkedAt,
      ready: status == 'ready',
      checks: checks,
    );
  }
}
