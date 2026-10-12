# ADR-0012: SDK Flutter en Dart puro sobre plugins de plataforma, sin núcleo Rust en v1

- **Estado:** Aceptado (baseline v0.2.0)
- **Fecha:** 2026-10-11
- **Responsable:** propietario del proyecto
- **Requisitos afectados:** FR-SDK-*, NFR-COMPAT-001

## Contexto

Un núcleo Rust compartido vía FFI reduciría divergencias entre SDKs futuros, pero obliga a cada integrador a cargar binarios nativos por arquitectura y complica la web. La lógica de cliente es pequeña; lo difícil es la capa de ubicación del sistema operativo, que es Kotlin/Swift en cualquier caso.

## Opciones consideradas

1. Dart puro + `geolocator` (y `permission_handler` si hace falta) + plugin propio mínimo solo si una capacidad no está disponible.
2. Núcleo Rust con FFI (p. ej. flutter_rust_bridge).
3. Plugins nativos completos desde el inicio.

## Decisión

Opción 1 para v1. `geolocator` como fuente de ubicación por defecto, inyectable. `permission_handler` solo si `geolocator` no cubre el flujo `always` (decisión registrada en este ADR durante T-F5-07). No usar plugins con licencia de pago en builds de release (p. ej. `flutter_background_geolocation` requiere licencia en iOS y Android), porque el costo recaería en cada integrador. Reevaluar un núcleo Rust al abordar SDKs nativos (F15).

## Consecuencias

+ Integración simple para el desarrollador; builds más rápidos; soporte web.
- Lógica de protocolo duplicada cuando existan SDKs nativos.

## Seguridad, privacidad y operación

Verificar licencia y versión de cada plugin en F5.

## Referencias

`12_FLUTTER_SDK.md`
