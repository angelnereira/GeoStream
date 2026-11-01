/// Estado de una dependencia de la API (`ReadinessStatus.checks`, contrato OpenAPI).
enum CheckState {
  /// Respondió a tiempo.
  ok,

  /// No respondió.
  failing,

  /// La instancia está drenando.
  draining,

  /// La configuración no usa esa dependencia.
  notConfigured;

  /// Interpreta el valor del contrato; `null` si no es uno de los cuatro.
  static CheckState? parse(Object? value) => switch (value) {
    'ok' => CheckState.ok,
    'failing' => CheckState.failing,
    'draining' => CheckState.draining,
    'not_configured' => CheckState.notConfigured,
    _ => null,
  };
}

/// Resultado de consultar el estado de la API.
sealed class ReadinessSnapshot {
  const ReadinessSnapshot(this.checkedAt);

  /// Hora de la consulta (UTC).
  final DateTime checkedAt;
}

/// La API respondió con un `ReadinessStatus` válido.
final class ReadinessKnown extends ReadinessSnapshot {
  /// Crea el resultado.
  const ReadinessKnown({
    required DateTime checkedAt,
    required this.ready,
    required this.checks,
  }) : super(checkedAt);

  /// `status == "ready"`.
  final bool ready;

  /// Estado por dependencia, por nombre.
  final Map<String, CheckState> checks;
}

/// No hay datos: el servicio no respondió o respondió algo que no es el contrato.
///
/// Nunca se interpreta como disponible (FR-PRT-002).
final class ReadinessUnknown extends ReadinessSnapshot {
  /// Crea el resultado.
  const ReadinessUnknown(super.checkedAt);
}
