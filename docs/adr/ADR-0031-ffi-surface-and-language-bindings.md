# ADR-0031: Herramientas de bindings por lenguaje y excepción de `unsafe` en `gs-ffi`

- **Estado:** Aceptado (elección final del generador Dart verificada con mediciones en el spike T-E3-01)
- **Fecha:** 2026-10-15
- **Responsable:** propietario del proyecto
- **Requisitos afectados:** FR-BND-001..007, NFR-COMPAT-003, NFR-MOB-004, NFR-SEC-005

## Contexto

La decisión **D-11** del propietario (`GS-ENG-A02` §3) requiere que el motor 100 % Rust se exponga mediante SDKs o adaptadores idiomáticos para **Kotlin (Android), Swift (iOS), Dart (Flutter), TypeScript (Web y Node), Go y Ruby**. Al mismo tiempo, `19_ENGINEERING_STANDARDS.md` §3 impone `#![forbid(unsafe_code)]` en todos los crates del workspace, pero toda frontera FFI hacia C/JNI/Swift/Dart/Node requiere bloques `unsafe` generados o de pegamento.

## Opciones consideradas

1. Permitir `unsafe` en cualquier crate del workspace cuando haga falta FFI (diluye la garantía de seguridad de memoria del motor).
2. Mantener clientes separados escritos en cada lenguaje sin FFI (descartado en ADR-0022).
3. **Aislar toda la frontera FFI en un único crate `gs-ffi` (única excepción autorizada a `#![forbid(unsafe_code)]`), diseñado sobre el modelo de interfaz de UniFFI y reutilizado por todos los generadores de lenguaje.**

## Decisión

Adoptar la Opción 3 (decisiones D-05 y D-11):
- **Excepción única de `unsafe` (`T-31` en `10_SECURITY_PRIVACY.md`):** todos los crates del servidor y del núcleo (`gs-domain`, `gs-geo`, `gs-track`, `gs-mobility`, `gs-analytics`, `gs-edge`, `gs-client-core`, `gs-client-runtime`, etc.) mantienen `#![forbid(unsafe_code)]` sin excepción. Solo `gs-ffi` y el código generado en `bindings/*` tienen permitido el código `unsafe` estrictamente necesario para cruzar la ABI, prohibiendo que cualquier `panic!` cruce la frontera FFI (`catch_unwind` convirtiendo a error tipado).
- **Herramientas de generación por lenguaje (§13.3):**
  - **Kotlin (Android/JVM), Swift (iOS/macOS) y Ruby:** `uniffi` (soporte oficial).
  - **Go:** `uniffi-bindgen-go` (o `cgo` sobre C ABI generada con `cbindgen` como fallback).
  - **Dart (Flutter):** se evalúa `flutter_rust_bridge` frente a `uniffi-rs-dart` / `dart:ffi` en el spike **T-E3-01** midiendo tamaño binario, latencia de arranque y ergonomía de streams.
  - **TypeScript (Node):** `napi-rs` (`napi` / `napi-derive`).
  - **TypeScript (Web):** `wasm-bindgen` compilando `gs-client-core` a `wasm32-unknown-unknown`.
- **Conformidad (`FR-BND-007`, `NFR-COMPAT-003`):** cada binding ejecuta el 100 % de los vectores dorados aplicables de `contracts/test-vectors/` en CI.

## Consecuencias

+ Superficie de auditoría mínima: un solo crate `gs-ffi` concentra toda la conversión de tipos y es sometido a `cargo-fuzz`.
+ Fachadas L2 idiomáticas en los 6 lenguajes sobre una sola implementación de negocio L0.

## Seguridad, privacidad y operación

Mitiga la amenaza T-31 (`10_SECURITY_PRIVACY.md`): ningún puntero crudo sale de `gs-ffi` hacia la lógica del motor, y toda llamada FFI atrapa pánicos y valida longitudes antes de construir tipos de dominio.

## Plan de migración

El spike T-E3-01 valida Android, iOS y Linux con Kotlin, Swift y Dart; E3 entrega los SDKs de dispositivo y E9 entrega Web, Node, Go y Ruby tras congelar el contrato v1.

## Evidencia y referencias

- Decisiones D-05 y D-11 en `docs/proposals/GeoStream_Addendum_A02_Motor_Ubicacion_Movilidad.md` §3 y §13.
- Documentación de UniFFI (`https://docs.rs/crate/uniffi/latest`, consultada 2026-10-15); `docs/26_CORE_AND_BINDINGS.md`, `docs/19_ENGINEERING_STANDARDS.md`.
