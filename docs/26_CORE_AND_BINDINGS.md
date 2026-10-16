# 26 — Núcleo compartido y bindings por lenguaje

**ID:** GS-CBN-001 · **Versión:** 0.3.0
Gobierna `gs-client-core`, `gs-client-runtime`, `gs-ffi` y los SDKs en `sdks/*`. Complementa `12_FLUTTER_SDK.md` (que conserva su API pública) y `19` §6 (dependencias).

## 1. Núcleo Rust compartido (`gs-client-core` + `gs-client-runtime`)

### 1.1 Diseño sans-IO

El núcleo **no hace I/O**: recibe entradas y devuelve acciones. Por eso el mismo código funciona en Android, iOS, web (WASM), escritorio y backends, y se prueba de forma determinista con relojes simulados (`20` §7).

```rust
// ILUSTRATIVO
pub struct ClientCore { /* estado: conexión, sesiones, secuencia, reloj, muestreo, cola, vistas */ }

impl ClientCore {
    pub fn handle(&mut self, input: Input, now_ms: u64) -> Outputs;  // sin I/O, sin bloqueo
    pub fn next_timer_ms(&self) -> Option<u64>;
}

pub enum Input {
    Command(Command),                 // start_sharing, stop_sharing, watch, set_intent…
    LocationFix(PlatformFix),         // desde el anfitrión (FusedLocation / CoreLocation / geolocator)
    Activity(PlatformActivity),       // reconocimiento de actividad, si hay permiso
    RegionExited(RegionId),           // despertar por región del sistema operativo
    BleSighting(BleAdvertisement),    // relevo de tags (E8)
    TransportOpened, FrameReceived(Vec<u8>), TransportClosed(u16),
    HttpResponse(RequestId, HttpResult),
    Connectivity(NetState), AppVisibility(bool), Timer,
}

pub enum Output {
    OpenTransport { url: String, subprotocols: Vec<String> }, SendFrame(Vec<u8>), CloseTransport(u16),
    Http(RequestId, HttpRequest),
    ConfigureLocation(LocationRequest),      // prioridad, intervalo, distancia mínima, retardo de lote
    MonitorRegion(Region), StopMonitoringRegion(RegionId),
    ConfigureBleScan(Option<BleScanConfig>),
    Persist { key: String, value: Vec<u8>, secure: bool },
    Emit(ClientEvent),                       // estados, vistas, errores, alertas de rastreo
}
```

- **`gs-client-runtime`** aporta un ejecutor nativo con `tokio`, WebSocket y HTTP para Android, iOS, escritorio y backends (Go, Ruby y Node mediante binding). En la web, el anfitrión TypeScript hace el transporte y el núcleo corre en WASM.
- **Todo lo que hoy especifica `12` §4–§15** se implementa en el núcleo con la misma semántica: estados, reconexión con jitter completo, tabla de cierres, `ClockSync`, `SequenceAllocator`, `SessionView`, redacción y muestreo/publicación/predicción/cola de `22` §3. La fachada de cada lenguaje solo traduce tipos.

### 1.2 Reglas del núcleo

1. `#![forbid(unsafe_code)]` obligatorio.
2. Sin `tokio`, sin red, sin sistema de archivos. Todo entra por `Input` y sale por `Output`.
3. Cero asignaciones por muestra en los algoritmos del camino caliente (`22` P-E2).
4. Compila a `wasm32-unknown-unknown` con el crate puro.
5. El cliente REST también es sans-IO (`Output::Http` / `Input::HttpResponse`), de modo que REST, stream, algoritmos y validación se implementan **una sola vez** (`FR-CORE-003`).

## 2. Matriz de objetivos de compilación

| Plataforma | Triples Rust | Empaquetado | CI |
|---|---|---|---|
| Servidor Linux x86_64 | `x86_64-unknown-linux-gnu`, `x86_64-unknown-linux-musl` | Binario e imagen de contenedor | Linux |
| Servidor Linux ARM64 | `aarch64-unknown-linux-gnu`, `aarch64-unknown-linux-musl` | Imagen multiarquitectura | Runner ARM64 o compilación cruzada verificada |
| Android | `aarch64-linux-android`, `armv7-linux-androideabi`, `x86_64-linux-android` (emulador) | AAR con `.so` por ABI | Linux + emulador |
| iOS | `aarch64-apple-ios`, `aarch64-apple-ios-sim` | XCFramework + Swift Package | **macOS obligatorio**; sin macOS la tarea queda abierta (`20` §8) |
| macOS | `aarch64-apple-darwin`, `x86_64-apple-darwin` | Bibliotecas para Swift, Go, Ruby y Node | macOS |
| Web | `wasm32-unknown-unknown` (solo crates puros) | Paquete npm con WASM | Linux |
| Windows (COULD) | `x86_64-pc-windows-msvc` | Para Node, Go y Ruby si se pide | Windows |

`FR-OPS-007` exige compilación multiarquitectura Linux verificada en CI (`T-E1-09`).

## 3. Capas de los SDK

- **L0 — Núcleo Rust:** `gs-client-core` + `gs-client-runtime` + librerías puras.
- **L1 — Bindings generados** sobre una única superficie `gs-ffi`, con tipos simples: registros, enums, interfaces de callback y errores tipados.
- **L2 — Fachada idiomática** por lenguaje: `Future`/`Stream` en Dart, `suspend`/`Flow` en Kotlin, `async`/`AsyncSequence` en Swift, `Promise`/`AsyncIterator` en TS, canales en Go, bloques en Ruby. Mantiene la API pública de `12` §4 para Dart y sus equivalentes.
- **L3 — Adaptadores de plataforma y mapas:** puertos de `22` §3.8 y renderers de `13`.

### 3.1 Familias de SDK

| Familia | Lenguajes | Qué incluye |
|---|---|---|
| Dispositivo | Kotlin (Android), Swift (iOS), Dart (Flutter), TypeScript (Web) | Todo `22` §3: muestreo, publicación, cola, predicción, BLE y segundo plano |
| Backend | Go, TypeScript (Node), Ruby | Cliente REST tipado, ayuda para emitir tokens (`POST /v1/tokens`), verificación de firma de webhooks, consumidor de stream del lado servidor (principal de aplicación), herramientas locales de `gs-mobility` (ETA estimado, asignación) y clientes de historial y analítica |

### 3.2 Herramientas de bindings (ADR-0031)

Estado consultado el 2026-10-15 en la documentación de UniFFI. Las versiones y licencias se verifican al redactar el ADR de la dependencia (`19` §6.1).

| Lenguaje | Herramienta propuesta | Estado | Alternativa si el spike falla |
|---|---|---|---|
| Kotlin (Android/JVM) | UniFFI | Soporte oficial | JNI manual sobre la C ABI |
| Swift (iOS/macOS) | UniFFI | Soporte oficial | C ABI + módulo Swift |
| Ruby | UniFFI | Soporte oficial | Gem FFI sobre la C ABI |
| Go | `uniffi-bindgen-go` (NordSecurity) | Tercero | cgo sobre una C ABI generada con `cbindgen` |
| Dart/Flutter | `flutter_rust_bridge` | Verificar versión y licencia | `uniffi-rs-dart` (tercero) o `dart:ffi` sobre la C ABI |
| Node (TypeScript) | `napi-rs` | Verificar | Generadores Node de UniFFI (tempranos) |
| Web (TypeScript) | `wasm-bindgen` | Verificar | — |
| React Native (COULD) | `uniffi-bindgen-react-native` | Tercero | — |

**Reglas**
- La superficie `gs-ffi` se diseña con el modelo de UniFFI; las demás herramientas envuelven **la misma** superficie.
- `unsafe` solo existe en `gs-ffi` y en el código generado. Esa superficie se somete a *fuzzing* (F10, `cargo-fuzz`, `NFR-SEC-005`).
- Ningún `panic` cruza la frontera FFI: se convierte en error tipado con `catch_unwind`.
- La licencia de cada herramienta se comprueba contra `19` §6.1. MPL-2.0 está permitida como dependencia sin modificar.
- El spike **T-E3-01** decide la herramienta de Dart con mediciones: tamaño, tiempo de arranque, ergonomía de streams y soporte de segundo plano.

## 4. Conformidad entre lenguajes (`FR-BND-007`, `NFR-COMPAT-003`)

Los mismos vectores dorados de `contracts/test-vectors/` y el subconjunto de escenarios aplicables se ejecutan en **Rust y en cada binding habilitado**, con tolerancias declaradas en el propio vector. CI falla si un binding habilitado no alcanza el 100 %.

Además, el arnés comprueba que ningún binding introduce coordenadas en logs: la captura de salida de cada-language se verifica con el mismo verificador de `gs-observability` (`18` §7).

## 5. Optimización del servidor Linux

Cada medida se adopta **solo con evidencia de benchmark** (`gs-loadgen`, perfiles P-M y P-L) registrada en `docs/benchmarks/`.

- **Perfil de release:** `lto` (`thin` o `fat`, según medición), `codegen-units = 1` y símbolos separados. `panic = "unwind"` se mantiene para aislar tareas de `tokio`, salvo evidencia en contra.
- **CPU:** imagen base `x86-64-v2`; imagen opcional `x86-64-v3`; imagen ARM64 para Graviton y Ampere; optimización guiada por perfil (PGO) como COULD.
- **Asignador:** comparar el de glibc con mimalloc y jemalloc en P-M. Uno nuevo solo entra con ADR y mejora medida.
- **Camino caliente:** búferes de codificación Protobuf reutilizados por conexión y cero asignaciones en `gs-track`. Se verifica con un contador de asignaciones en los benchmarks.
- **Red y kernel:** `TCP_NODELAY` en WebSocket. En `16` se documentan los ajustes de `sysctl` y límites de descriptores, con los valores usados en la medición.
- **Contenedor:** imagen mínima no-root multiarquitectura.
- **No adoptar sin evidencia:** `io_uring`, runtimes alternativos ni SIMD manual. La pila `tokio` se mantiene.

## 6. Referencias

- `03` §2.20 (FR-CORE-*, FR-BND-*), §2.18 (`FR-OPS-007`), §3 (NFR-MOB-004/005, NFR-COMPAT-003, NFR-SEC-005).
- `19` §6 (dependencias aprobadas por ADR), `18` §5 (conformidad de contratos y SDK).
- ADR-0022 (núcleo compartido), ADR-0031 (bindings y excepción de `unsafe`).
