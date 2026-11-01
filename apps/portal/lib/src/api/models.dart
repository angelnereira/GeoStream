/// Resumen de una sesión (`Session` del contrato OpenAPI) con lo que muestra la consola.
final class SessionSummary {
  /// Crea el resumen.
  const SessionSummary({
    required this.id,
    required this.mode,
    required this.status,
    required this.activeMemberCount,
    required this.expiresAt,
  });

  /// Lee una sesión del JSON del contrato; `null` si falta un campo obligatorio.
  static SessionSummary? fromJson(Object? json) {
    if (json is! Map<String, Object?>) return null;
    final id = json['id'];
    final mode = json['mode'];
    final status = json['status'];
    final count = json['active_member_count'];
    final expires = json['expires_at'];
    if (id is! String ||
        mode is! String ||
        status is! String ||
        count is! int ||
        expires is! String) {
      return null;
    }
    return SessionSummary(
      id: id,
      mode: mode,
      status: status,
      activeMemberCount: count,
      expiresAt: DateTime.tryParse(expires)?.toUtc(),
    );
  }

  /// Identificador público (`ses_…`).
  final String id;

  /// Modo de la sesión (`ONE_TO_ONE`, `MANY_TO_MANY`…).
  final String mode;

  /// Estado (`ACTIVE`, `CLOSED`, `EXPIRED`).
  final String status;

  /// Miembros activos.
  final int activeMemberCount;

  /// Expiración, o `null` si no se pudo interpretar.
  final DateTime? expiresAt;
}

/// Última posición retenida de un miembro (`SnapshotItem` del contrato).
final class MemberPosition {
  /// Crea la posición.
  const MemberPosition({
    required this.memberId,
    required this.label,
    required this.latitude,
    required this.longitude,
    required this.accuracyMeters,
    required this.ageMs,
  });

  /// Lee un elemento del snapshot; `null` si falta algo o está fuera de rango.
  static MemberPosition? fromJson(Object? json) {
    if (json is! Map<String, Object?>) return null;
    final memberId = json['member_id'];
    final subject = json['external_subject_id'];
    final age = json['age_ms'];
    final sample = json['sample'];
    if (memberId is! String || subject is! String || age is! int) return null;
    if (sample is! Map<String, Object?>) return null;
    final position = sample['position'];
    if (position is! Map<String, Object?>) return null;
    final lat = position['latitude'];
    final lon = position['longitude'];
    final accuracy = position['horizontal_accuracy_m'];
    if (lat is! num || lon is! num || accuracy is! num) return null;
    if (lat < -90 || lat > 90 || lon < -180 || lon > 180) return null;
    return MemberPosition(
      memberId: memberId,
      label: subject,
      latitude: lat.toDouble(),
      longitude: lon.toDouble(),
      accuracyMeters: accuracy.toDouble(),
      ageMs: age,
    );
  }

  /// Identificador del miembro.
  final String memberId;

  /// Identificador opaco que puso el integrador (`external_subject_id`).
  final String label;

  /// Latitud WGS 84 en grados.
  final double latitude;

  /// Longitud WGS 84 en grados.
  final double longitude;

  /// Precisión horizontal en metros.
  final double accuracyMeters;

  /// Antigüedad de la muestra en milisegundos cuando el servidor generó el snapshot.
  final int ageMs;
}
