# ADR-0022: Núcleo Rust compartido entre servidor y dispositivo (sans-IO + runtime)

- **Estado:** Aceptado
- **Fecha:** 2026-10-15
- **Responsable:** propietario del proyecto
- **Requisitos afectados:** FR-CORE-001..003, FR-SMP-001..006, FR-BND-001..007, FR-SDK-001..016
- **Sustituye a:** ADR-0012

## Contexto

ADR-0012 había optado por implementar el SDK Flutter en Dart puro para evitar compilación nativa en v1. Sin embargo, las decisiones **D-01, D-04, D-05 y D-11** exigen que el motor opere en el dispositivo con muestreo adaptativo, puerta de calidad, publicación por cambio significativo, predicción visual, cola offline cifrada y relevo BLE, y que además dé soporte a Kotlin (Android), Swift (iOS), Dart (Flutter), TypeScript (web y Node), Go y Ruby sin duplicar la lógica del protocolo ni los algoritmos matemáticos en seis lenguajes.

## Opciones consideradas

1. Reimplementar protocolo, validación, muestreo adaptativo, filtro local, predicción y criptografía BLE en cada lenguaje (Dart, Kotlin, Swift, TypeScript, Go, Ruby).
2. Núcleo Rust compartido con I/O acoplado a `tokio` en todas partes (impide compilar a `wasm32-unknown-unknown` para navegadores web y dificulta las pruebas deterministas con reloj simulado).
3. **Núcleo Rust compartido en dos capas:** `gs-client-core` puro **sans-IO** (sin `tokio` ni red, compilable a `wasm32` y 100 % determinista) + `gs-client-runtime` nativo (`tokio`, WebSocket y HTTP para Android, iOS, escritorio y backends), expuesto mediante una superficie FFI única (`gs-ffi`) bajo anfitriones delgados por plataforma.

## Decisión

Adoptar la Opción 3 (decisiones D-01, D-05, D-11), sustituyendo a ADR-0012:
- `gs-client-core` implementa una sola vez en Rust toda la lógica de `12_FLUTTER_SDK.md` §4–§15 y `22_TRACKING_ENGINE.md`: máquina de estados de conexión, reconexión con jitter, tabla de cierres, `ClockSync`, `SequenceAllocator`, cliente REST sans-IO (`Output::Http`), muestreo adaptativo, publicación por cambio significativo, cola `latestOnly`/`buffered`, `SessionView` con predicción visual (`renderPosition`) y relevo BLE.
- `gs-client-runtime` provee el ejecutor nativo para Android, iOS y SDKs de backend. En web, el anfitrión TypeScript ejecuta el transporte y el núcleo corre en WASM.
- La fachada pública de `12_FLUTTER_SDK.md` §4 en Dart se mantiene intacta como capa L2 delgada sobre el núcleo.
- El soporte de iOS y la elección final del generador Dart se validan con mediciones reales en el spike **T-E3-01**.

## Consecuencias

+ Paridad matemática y de protocolo garantizada entre servidor y todos los SDKs mediante los mismos vectores dorados (`P-E1`, `P-E6`).
+ Pruebas deterministas instantáneas del cliente sin red ni temporizadores reales gracias al diseño sans-IO.
- Requiere toolchain de compilación cruzada (Android NDK, Xcode/macOS para iOS, `wasm32`) validada desde T-E3-01.

## Seguridad, privacidad y operación

La redacción de coordenadas y secretos (`Position`, `AccessToken`, `Ticket`) se aplica en el núcleo Rust antes de cruzar a cualquier lenguaje anfitrión. El tamaño binario por ABI se controla mediante `NFR-MOB-004` (≤ 4 MB stripped).

## Plan de migración

Como F5 aún no había iniciado código en `sdks/flutter`, la sustitución de ADR-0012 no descarta código existente y re-alcance F5 hacia la fase E3 (`21_ROADMAP.md`).

## Evidencia y referencias

- Decisiones D-01, D-05, D-11 en `docs/proposals/GeoStream_Addendum_A02_Motor_Ubicacion_Movilidad.md` §3, §8 y §13.
- `docs/12_FLUTTER_SDK.md`, `docs/26_CORE_AND_BINDINGS.md`, ADR-0031.
