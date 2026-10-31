/// Persistencia local: secuencia (ADR-0008), URL del servidor y geocercas.
///
/// El token de dispositivo **no** se persiste: vive solo en memoria.
library;

import 'dart:convert';

import 'package:shared_preferences/shared_preferences.dart';

import '../geo/geofence.dart';

/// Almacén clave-valor mínimo, inyectable en pruebas.
abstract interface class KeyValueStore {
  /// Lee un entero.
  int? getInt(String key);

  /// Escribe un entero.
  Future<void> setInt(String key, int value);

  /// Lee una cadena.
  String? getString(String key);

  /// Escribe una cadena.
  Future<void> setString(String key, String value);
}

/// [KeyValueStore] sobre `shared_preferences`.
final class PrefsStore implements KeyValueStore {
  /// Envuelve una instancia ya cargada.
  PrefsStore(this._prefs);

  final SharedPreferences _prefs;

  /// Carga las preferencias de la plataforma.
  static Future<PrefsStore> load() async =>
      PrefsStore(await SharedPreferences.getInstance());

  @override
  int? getInt(String key) => _prefs.getInt(key);

  @override
  Future<void> setInt(String key, int value) => _prefs.setInt(key, value);

  @override
  String? getString(String key) => _prefs.getString(key);

  @override
  Future<void> setString(String key, String value) =>
      _prefs.setString(key, value);
}

/// [KeyValueStore] en memoria para pruebas.
final class MemoryStore implements KeyValueStore {
  final Map<String, Object> _data = {};

  @override
  int? getInt(String key) => _data[key] as int?;

  @override
  Future<void> setInt(String key, int value) async => _data[key] = value;

  @override
  String? getString(String key) => _data[key] as String?;

  @override
  Future<void> setString(String key, String value) async => _data[key] = value;
}

/// Asignador de secuencias por bloques (`docs/12_FLUTTER_SDK.md` §9).
///
/// Reserva `block` valores y persiste la reserva **antes** de usarlos, de modo
/// que un cierre abrupto deja huecos pero nunca repite una secuencia.
final class SequenceAllocator {
  SequenceAllocator._(this._store, this._next, this._reserved, this.block);

  /// Clave de la reserva (`12` §9).
  static const String reservedKey = 'gs.v1.seq_reserved';

  final KeyValueStore _store;
  final int block;
  int _next;
  int _reserved;

  /// Abre el asignador sobre [store], reservando el primer bloque.
  static Future<SequenceAllocator> open(
    KeyValueStore store, {
    int block = 100,
  }) async {
    final next = store.getInt(reservedKey) ?? 1;
    final reserved = next + block;
    await store.setInt(reservedKey, reserved);
    return SequenceAllocator._(store, next, reserved, block);
  }

  /// Devuelve la siguiente secuencia, reservando otro bloque si hace falta.
  Future<int> next() async {
    if (_next >= _reserved) {
      _reserved = _next + block;
      await _store.setInt(reservedKey, _reserved);
    }
    return _next++;
  }
}

/// Ajustes y geocercas de la app.
final class AppStore {
  /// Crea el almacén sobre [kv].
  AppStore(this.kv);

  /// Almacén subyacente.
  final KeyValueStore kv;

  static const String _baseUrlKey = 'tester.base_url';
  static const String _subjectKey = 'tester.external_subject_id';
  static const String _fencesKey = 'tester.geofences';

  /// URL por defecto: el despliegue de Railway en gravitalcloud.com.
  static const String defaultBaseUrl = 'https://geostream.gravitalcloud.com';

  /// URL base del servidor.
  String get baseUrl => kv.getString(_baseUrlKey) ?? defaultBaseUrl;

  /// Guarda la URL base.
  Future<void> setBaseUrl(String value) => kv.setString(_baseUrlKey, value);

  /// Último `external_subject_id` informado (solo para mostrarlo).
  String get externalSubjectId => kv.getString(_subjectKey) ?? '';

  /// Guarda el `external_subject_id`.
  Future<void> setExternalSubjectId(String value) =>
      kv.setString(_subjectKey, value);

  /// Geocercas guardadas; ignora entradas corruptas.
  List<LocalGeofence> loadGeofences() {
    final raw = kv.getString(_fencesKey);
    if (raw == null) return [];
    try {
      final list = jsonDecode(raw) as List<dynamic>;
      return [
        for (final e in list.cast<Map<String, dynamic>>())
          LocalGeofence.fromJson(e),
      ];
    } on Object {
      return [];
    }
  }

  /// Persiste [fences].
  Future<void> saveGeofences(List<LocalGeofence> fences) => kv.setString(
    _fencesKey,
    jsonEncode([for (final f in fences) f.toJson()]),
  );
}
