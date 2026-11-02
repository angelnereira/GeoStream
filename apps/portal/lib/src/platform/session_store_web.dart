import 'dart:js_interop';

import 'session_store_base.dart';

extension type _Storage._(JSObject _) implements JSObject {
  external String? getItem(String key);
  external void setItem(String key, String value);
  external void removeItem(String key);
}

@JS('sessionStorage')
external _Storage get _storage;

const String _key = 'gs.console.session';

/// Guarda el token de sesión en `sessionStorage` (ADR-0037): sobrevive a recargar la página, pero no al cierre
/// de la pestaña, y no se comparte con otras pestañas. Ningún otro almacenamiento persistente ni cookies.
///
/// El navegador puede negar el acceso (modo privado, datos bloqueados): en ese caso la sesión simplemente no
/// sobrevive a la recarga, sin romper nada.
class WebSessionStore implements SessionStore {
  /// Crea el almacén.
  const WebSessionStore();

  @override
  String? read() {
    try {
      return _storage.getItem(_key);
    } on Object {
      return null;
    }
  }

  @override
  void write(String token) {
    try {
      _storage.setItem(_key, token);
    } on Object {
      // Sin almacenamiento: la sesión vive solo en memoria.
    }
  }

  @override
  void clear() {
    try {
      _storage.removeItem(_key);
    } on Object {
      // Nada que borrar.
    }
  }
}

/// El almacén de la plataforma: `sessionStorage` en el navegador.
SessionStore createSessionStore() => const WebSessionStore();
