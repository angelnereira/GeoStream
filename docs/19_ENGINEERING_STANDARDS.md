# 19 — Estándares de ingeniería (desarrolladores y agentes)

**ID:** GS-ENG-001 · **Versión:** 0.3.0
Obligatorios para todo cambio. Una excepción requiere ADR o justificación escrita aprobada en el PR, según indique cada regla.

## 1. Definition of Done (toda tarea)

Una tarea está terminada solo si:
1. Implementa los requisitos citados por la tarea y nada que no esté especificado.
2. Tiene pruebas que cubren cada criterio de aceptación citado (`covers:` con el ID).
3. Pasan **localmente y en CI** todas las etapas obligatorias (`18` §10.2).
4. La documentación y los contratos afectados están actualizados en el mismo PR.
5. `CHANGELOG.md` actualizado si hay cambio visible (API, protocolo, configuración, comportamiento, SDK).
6. `TRACEABILITY.md` y `PROGRESS.md` actualizados.
7. No introduce dependencias fuera de la lista aprobada (§6) sin ADR.
8. Si toca autenticación, autorización, datos, red, persistencia, criptografía o logs: la sección relevante de `10` fue revisada y el PR lo declara.
9. El informe de tarea (`docs/templates/TASK_REPORT.md`) está en la descripción del PR con comandos ejecutados y resultados reales.

"Compila" no es "terminado".

### 1.1 DoD adicional de la pista E

Para cualquier tarea de fase `E<n>`, la Definition of Done anterior se amplía con:

10. **Vectors.** Toda constante nueva del motor aparece en `17` y tiene al menos un vector en `contracts/test-vectors/` que fija su comportamiento. Sin vector, el valor no está fijado.
11. **Redacción verificada.** Si la tarea toca coordenadas, celdas H3, estimaciones, identificadores de tag o credenciales de proveedor, la suite `gs-testkit --test privacy_test` pasa y el PR lo dice.
12. **Degradación declarada.** Si la tarea introduce un componente opcional (proveedor, grafo, etiquetas, grafo vial), el comportamiento cuando **no está disponible** está escrito y probado. Una capacidad opcional sin degradación declarada es una capacidad obligatoria disfrazada.
13. **Presupuesto.** Si la tarea añade trabajo al camino de aceptación (publicación → distribución), hay un perfil de carga de §8.3 y un informe con números reales. «Probablemente sea rápido» no es una medida.
14. **Compatibilidad.** Si la tarea cambia un contrato, hay una prueba de que un cliente de la versión anterior sigue funcionando (`NFR-COMPAT-003`) y el campo nuevo es opcional o tiene número de campo libre.
15. **Revisión de privacidad.** Si la tarea cambia qué ve cada parte (`10` §6.3) o qué se conserva (§6.2), la tabla correspondiente está actualizada en el mismo PR.

### 1.2 DoD por lenguaje de binding (E9)

Para los SDK que usan FFI:

- La superficie pública del binding es estable y versionada; cambiar su firma es un cambio mayor del SDK.
- Existe una prueba de **conformidad cruzada**: los mismos vectores se ejecutan en el lenguaje y en el núcleo Rust, y las salidas coinciden según la matriz de tolerancias de `26` §4.
- La aplicación de ejemplo compila para el objetivo mínimo soportado (`26` §2), no solo para el de desarrollo.
- Un `panic` en el núcleo produce un error tipado en el lenguaje, no un cierre de proceso.
- La documentación del paquete explica cómo se inicializa el runtime y qué se hace con los recursos al disposing, en ese lenguaje.

## 2. Reglas generales de código
- Simplicidad: módulos cohesivos, sin abstracciones genéricas prematuras ni capas sin uso.
- Nombres en inglés; comentarios de código en inglés; documentación en `docs/` en español.
- Sin código muerto, sin `TODO` sin issue enlazado (`TODO(#123)`).
- Sin valores mágicos: todo número configurable sale de `gs-config` con el nombre de `17`.
- Nada de lógica de negocio en handlers HTTP/WS: viven en `gs-service` y `gs-domain`.

## 3. Rust
- Toolchain fijada en `rust-toolchain.toml` (stable vigente al iniciar F0); edición 2024; MSRV = toolchain fijada.
- `#![forbid(unsafe_code)]` en todos los crates. Excepción solo con ADR.
- `cargo fmt` sin cambios; `cargo clippy --workspace --all-targets --all-features -- -D warnings`.
- Errores: `thiserror` en librerías con enums de error tipados; `anyhow` solo en `apps/` y pruebas. Los errores de dominio se mapean explícitamente a `ErrorCode` (`09` §6) en una sola función por frontera.
- Prohibido en código de producción: `unwrap()`, `expect()`, `panic!`, `unreachable!`, indexación que pueda entrar en pánico, salvo invariante documentado en comentario `// INVARIANT:` y revisado. En pruebas están permitidos.
- Prohibido `let _ = …` sobre un `Result` sin comentario que justifique por qué es seguro ignorarlo.
- Async:
  - Nunca mantener un `MutexGuard` (std o `parking_lot`) a través de un `.await`; usar `tokio::sync` solo si es imprescindible.
  - Todo canal acotado (`mpsc::channel(n)`), nunca `unbounded_channel`.
  - Todo I/O de red o base de datos con timeout explícito.
  - Tareas lanzadas con `tokio::spawn` registradas y cancelables (`CancellationToken`/`JoinSet`); sin tareas huérfanas.
  - Sin bloqueo en el runtime (`spawn_blocking` para CPU intensivo o I/O bloqueante).
- Logs con `tracing` y campos estructurados; nunca `println!` fuera de la CLI.
- Tipos con coordenadas o secretos: `Debug`/`Display` redactados (`10` §8.2).
- Documentación `///` en todo ítem público de las librerías.
- Features de Cargo: mínimas; las opcionales (p. ej. `otlp`) documentadas.
- Consultas SQL solo en `gs-store`; con `sqlx::query!`/`query_as!` y datos offline versionados (`.sqlx/`, `cargo sqlx prepare`) o consultas en tiempo de ejecución con pruebas de integración que las cubran.

## 4. Dart / Flutter
- `dart format`; `flutter analyze` sin infos/warnings con `flutter_lints` (o `lints/recommended`) más `strict-casts`, `strict-inference`, `strict-raw-types`.
- API pública tipada, asíncrona, con modelos inmutables propios; sin exponer tipos de Protobuf, `http` ni de WebSocket.
- Cerrar todo `StreamSubscription`, `Timer`, socket y controlador en `dispose`; pruebas que lo verifiquen.
- Sin `print`; usar el `GeoStreamLogger` inyectado.
- Sin secretos de servidor en el paquete ni en el ejemplo (la app de ejemplo obtiene tokens de un backend de ejemplo).
- Código nativo (si llega a ser necesario) en Kotlin/Swift, con ADR (`12` §12.3).

## 5. SQL y migraciones
Ver `11` §2 y §4. Toda consulta filtra por `tenant_id`. Nada de `SELECT *` en código de producción.

## 6. Política de dependencias

### 6.1 Reglas
- Solo dependencias de la lista aprobada. Añadir una requiere ADR con: necesidad, alternativas, licencia, mantenimiento (actividad, mantenedores), seguridad (advisories), tamaño y estrategia de salida.
- Versiones exactas fijadas por `Cargo.lock` / `pubspec.lock` (versionados). Actualizaciones en PRs dedicados.
- Licencias permitidas (verificadas por `cargo deny`): MIT, Apache-2.0, BSD-2-Clause, BSD-3-Clause, ISC, Zlib, Unicode-3.0, CC0-1.0; MPL-2.0 permitida como dependencia sin modificar. Cualquier otra → ADR (excepciones aprobadas en ADR-0020 para dependencias transitivas fijadas: `xxhash-rust` BSL-1.0, `webpki-root-certs` y `webpki-roots` CDLA-Permissive-2.0, `borrow-or-share` MIT-0).
- **No inventar APIs**: antes de usar una función de un crate o paquete, consultar su documentación **en la versión fijada** (docs.rs / pub.dev / código fuente) y compilar un ejemplo mínimo si hay dudas.

### 6.2 Lista aprobada — Rust

| Propósito | Crates |
|---|---|
| Runtime y web | `tokio`, `tokio-util`, `futures`, `axum` (feature `ws`), `tower`, `tower-http`, `http` |
| Serialización | `serde`, `serde_json`, `toml` |
| Protobuf | `prost`, `prost-types`, `prost-build`, `protoc-bin-vendored`, `pbjson`, `pbjson-types`, `pbjson-build` |
| PostgreSQL | `sqlx` (postgres, runtime tokio, rustls, uuid, time, json, migrate, macros) |
| Redis/Valkey | `redis` (tokio, connection manager, scripts) |
| Tiempo e IDs | `time`, `uuid` (v4, v7) |
| Errores | `thiserror`, `anyhow` (solo apps/pruebas) |
| Observabilidad | `tracing`, `tracing-subscriber`, `metrics`, `metrics-exporter-prometheus`; opcional `opentelemetry`, `opentelemetry-otlp`, `tracing-opentelemetry` |
| Seguridad | `jsonwebtoken`, `ed25519-dalek`, `sha2`, `hmac`, `subtle`, `rand`, `getrandom`, `aes-gcm`, `base64`, `zeroize` |
| Configuración y CLI | `clap` (derive, env), `figment` (toml, env) |
| HTTP saliente (webhooks) | `reqwest` (rustls, sin features por defecto) |
| Red | `ipnet` |
| Concurrencia | `dashmap` |
| Geoespacial (F11) | `geo`, `rstar` |
| **Geoespacial indexado (E2/E4)** | `h3o` (celdas H3, ADR-0026) |
| **Geometría de trayectorias (E2/E6)** | `geo` (ya aprobado), `geographiclib-rs` solo si `geo` no cubre lo requerido — decisión por ADR |
| **Criptografía de tags (E8)** | `p256` (o `x25519-dalek` para ECDH si el hardware no acelera P-256; decisión por ADR), `hkdf`, `chacha20poly1305` (si el beacon no puede hacer AES-GCM), `rand_core` |
| **Serialización de etiquetas (E6/E8)** | `csv`, `postcard` (para el `.gsg` de grafos) |
| **Asignación (E5)** | Implementación propia sobre `petgraph` (Hungarian y min-cost-flow; ~600 líneas) o `good_lp`; decisión registrada en ADR antes de la primera importación |
| Pruebas | `proptest`, `testcontainers`, `testcontainers-modules`, `tokio-tungstenite`, `jsonschema`, `criterion` (benchmarks) |
| Fuzzing (F10) | `libfuzzer-sys` (vía `cargo-fuzz`) |

El OpenAPI se convierte a JSON en CI (`redocly bundle … --ext json`) para que las pruebas lo lean sin un parser YAML.

**Criterio de importación de la pista E.** Las dependencias de arriba no están aprobadas de forma incondicional: cada una pasa por ADR en su tarea, con el mismo formato de §6.1. La razón es que la pista E multiplica el número de crates del workspace y que el coste de salir de una dependencia es proporcional a lo lejos que esté de la lista aprobada.

### 6.2.1 Reglas de `unsafe` y de la frontera FFI (E9)

- `#![forbid(unsafe_code)]` se mantiene **en todo el workspace**, con **una única excepción**: `crates/gs-ffi`, que declara `#![deny(unsafe_code)]` y una lista explícita de módulos permitidos a usarlo.
- Todo bloque `unsafe` en `gs-ffi` lleva un comentario `// SAFETY:` que justifique por qué se mantienen los invariantes. Sin ese comentario, el clippy falla.
- Ningún `panic` cruza la frontera: todo punto de entrada valida longitudes antes de construir tipos de dominio y usa `catch_unwind` para convertir un pánico en un error tipado.
- La superficie pública de la FFI es **estrecha por construcción**: tipos opacos con handle, funciones `extern "C"` sin conversiones implícitas, y `#[no_mangle]` solo en el módulo de la frontera.
- Revisión obligatoria de cambios en `gs-ffi` por una segunda persona, y fuzzing nocturno de su superficie (`18` §10.4). Es el único punto del sistema donde un fallo de memoria puede tumbar a un proceso ajeno.

### 6.2.2 Reglas de los crates nuevos

| Crate | Regla específica |
|---|---|
| `gs-geo` | Puro y sin I/O. Único sitio con fórmulas geodésicas, unidades explícitas y radio terrestre declarado. |
| `gs-track` | Puro salvo el acceso al almacén efímero, que se inyecta como puerto. `tokio` permitido solo para el bucle; la lógica del filtro es sincrónica y testeable sin runtime. |
| `gs-mobility` | Puro salvo el índice (trait `SpatialIndex` inyectable). La implementación en memoria y la de Redis cumplen el mismo contrato y se prueban con la misma suite. |
| `gs-history` | Solo escribe. Nunca lee para decidir nada en el camino de aceptación. |
| `gs-analytics` | Solo lee. Produce agregados; jamás escribe datos personales. |
| `gs-client-core` | Sin I/O. Sin `tokio`. Sin dependencias de plataforma. |
| `gs-client-runtime` | Único crate del cliente con dependencias de plataforma; implementa los puertos de `gs-client-core`. |
| `gs-ffi` | Único crate con `unsafe`. No contiene lógica de negocio. |

**Regla de pureza.** Ninguno de los crates de la pista E puede depender de `tokio`, `sqlx`, `axum`, `redis` o `reqwest` en su capa de núcleo; solo en su adaptador. El verificador `crate-deps` de `tools/` lo hace cumplir de forma automática (`04` §4.1).

### 6.3 Lista aprobada — Dart/Flutter

| Propósito | Paquetes |
|---|---|
| Red | `http`, `web_socket_channel` |
| Protobuf | `protobuf`, `fixnum`; herramienta `protoc_plugin` |
| Plataforma | `geolocator`, `connectivity_plus`, `shared_preferences`; `permission_handler` solo si `geolocator` no cubre el flujo `always` (decidir en T-F5-07 y registrar en ADR-0012) |
| Utilidades | `meta`, `collection` |
| Mapas (F9, paquetes separados) | `maplibre_gl`, `google_maps_flutter` |
| Pruebas | `test`, `flutter_test`, `mocktail`, `fake_async`, `integration_test` |
| Lint | `flutter_lints` o `lints` |

### 6.4 Herramientas (versiones fijadas en F0)
`buf`, `@redocly/cli`, `oasdiff`, `cargo-deny`, `cargo-audit`, `cargo-llvm-cov`, `cargo-fuzz`, `sqlx-cli`, `protoc` + `protoc_plugin` (Dart), Docker. Scripts de `tools/`: Python 3 solo con biblioteca estándar, o Rust.

## 7. Git y pull requests
- Rama principal: `main`, protegida; solo se fusiona vía PR con CI verde.
- Ramas: `<tipo>/<T-Fx-NN>-<descripcion-corta>` (p. ej. `feat/T-F4-05-publish-pipeline`).
- Commits: Conventional Commits (`feat:`, `fix:`, `docs:`, `test:`, `refactor:`, `chore:`, `perf:`, `ci:`), con ámbito opcional (`feat(stream): …`) y referencia a la tarea en el cuerpo.
- Un PR = una tarea del roadmap (o una parte claramente delimitada). Objetivo: < 600 líneas cambiadas sin contar código generado ni vectores.
- Fusión por *squash*. El título del PR sigue Conventional Commits.
- Plantilla obligatoria: `.github/pull_request_template.md`.
- Prohibido: `--no-verify`, desactivar o marcar como ignoradas pruebas para que CI pase, bajar umbrales de cobertura, editar contratos o vectores para que una implementación pase sin decisión documentada.

## 8. Versionado y releases
- SemVer independiente para: servidor (`server-vX.Y.Z`), contratos (OpenAPI `info.version` y `protocol_minor`), SDK Flutter (`sdk-flutter-vX.Y.Z`) y adaptadores de mapas.
- Antes de 1.0 se usan prereleases (`-alpha.N`, `-beta.N`, `-rc.N`).
- `CHANGELOG.md` en formato *Keep a Changelog*, con secciones `Added`, `Changed`, `Deprecated`, `Removed`, `Fixed`, `Security`, `Docs`.
- Deprecaciones: anunciadas en changelog y cabeceras (`07` §11) con ≥ 6 meses antes de retirar.
- Cambios incompatibles de protocolo o API pública: nueva versión mayor y aprobación explícita del propietario; nunca desplegados automáticamente.
- Release: checklist de `10` §11 + benchmarks (desde F6) + checklist de dispositivos (desde F5).

## 9. Documentación
- Reglas de `00` §1 y §5.
- Diagramas Mermaid para flujos con concurrencia, seguridad o ciclo de vida.
- Ejemplos de código: compilan o están rotulados `ILUSTRATIVO`.
- Afirmaciones de rendimiento: distinguir **objetivo**, **medido** y **SLA**.
- Toda clave de configuración nueva: añadirla a `17` con default, rango y efecto.

## 10. Checklist de revisión de código
- [ ] ¿Cada cambio de comportamiento está especificado y probado?
- [ ] ¿La identidad sale solo de la credencial? ¿Se filtra por `tenant_id`?
- [ ] ¿Hay coordenadas, tokens o secretos que puedan llegar a logs, métricas, trazas o errores?
- [ ] ¿Canales acotados, timeouts y cancelación correctos?
- [ ] ¿Errores mapeados al catálogo, sin detalles internos al cliente?
- [ ] ¿Límites configurables respetados con nombres de `17`?
- [ ] ¿Contratos y documentación actualizados?
- [ ] ¿Dependencias nuevas aprobadas?

## 11. Prohibiciones consolidadas
- Coordenadas, tokens, tickets, API keys, secretos o códigos de invitación en logs, trazas, métricas, crash reports o mensajes de error.
- Secretos en el repositorio, imágenes o apps.
- Publicar la ubicación de un usuario por defecto al unirse a una sesión.
- Presentar datos estimados o interpolados como medidos.
- Kubernetes, Kafka, P2P, map-matching, Tile38 o H3 como dependencia obligatoria sin requisito y ADR.
- Afirmar que una dependencia o servicio es gratuito o compatible sin revisar licencia y términos.
- Criptografía propia.
- Cerrar una tarea porque compila.
