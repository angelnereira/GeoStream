/// Dónde recuerda el portal el token de la sesión de consola entre recargas.
abstract interface class SessionStore {
  /// El token guardado, o `null`.
  String? read();

  /// Guarda el token.
  void write(String token);

  /// Olvida el token.
  void clear();
}

/// Almacén en memoria: pruebas y plataformas sin navegador.
class MemorySessionStore implements SessionStore {
  /// Crea el almacén, con un token inicial opcional.
  MemorySessionStore([this._token]);

  String? _token;

  @override
  String? read() => _token;

  @override
  void write(String token) => _token = token;

  @override
  void clear() => _token = null;
}
