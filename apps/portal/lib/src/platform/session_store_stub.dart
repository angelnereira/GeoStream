import 'session_store_base.dart';

/// El almacén de la plataforma fuera del navegador: solo memoria.
SessionStore createSessionStore() => MemorySessionStore();
