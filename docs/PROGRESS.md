# Bitácora de avance

**ID:** GS-PRG-001 · La mantienen los agentes y desarrolladores en cada PR (`20_AGENT_PROTOCOL.md` §3, §4).

## Estado actual

| Campo | Valor |
|---|---|
| Fase activa | **E0 — Documentación, contratos y ADR** de la pista E (motor de ubicación y movilidad) |
| Último hito alcanzado | **M1 — Cierre de Fase F4** (stream en nodo único) — 2026-10-15 |
| Paquete documental | v0.3.0 (2026-10-15), con la adopción del addendum A-02 |
| Siguiente fase | **E1 — Núcleo Rust compartido** (`gs-geo`, `gs-client-core`, `gs-client-runtime`, `gs-ffi`), lista para empezar |
| Decisiones pendientes | ADR-0019 (licencia) — no bloquea la implementación; bloquea publicar paquetes. Ninguna otra: las cuatro DR del addendum A-02 están **aceptadas** con Opción 1 (2026-10-15) |
| Disponibilidad de nombres | crates.io (`geostream`, `gs-protocol`, `geostream-sdk`, `geostream-server`) y pub.dev (`geostream`, `geostream_flutter`): todos disponibles (verificado 2026-10-12) |

## Tareas en curso

Ninguna. La fase E0 está completa y a la espera de revisión del propietario.

| Tarea | Estado |
|---|---|
| T-E0-01 … T-E0-08 | completadas (Fase E0) |
| T-E1-01 — `gs-geo` | completada | habilita E2, E3, E5 y E7 |
| T-E1-02 — `gs-client-core` | completada | habilita E3 |
| T-E1-03 — `gs-client-runtime` | completada | — |
| T-E1-04 — `gs-ffi` | completada | — |
| T-E1-05 — CI multiarch | parcialmente (workflow añadido) | wasm + musl |
| T-E1-06 — propiedades | completada | 4 propiedades |
| **T-F15-01 — adopción de la consola** | completada (documental) | DR-20261031-05, ADR-0033, FR-CON-*, T-32 a T-37, `17` §2.12. Siguiente: T-F15-02 (externa, Plenty-Pay) y T-F15-03 (contrato) |
| **T-F6-01 — Redis/Valkey** | **hecha en local; cierre pendiente de CI** (Angel Nereira con agente, 2026-10-31) | Backends, suites y cableado en `serve`. No se puede cerrar según `19` §1.3 mientras GitHub Actions no ejecute (ver bloqueos). Siguiente en F6: T-F6-02 (desplazamiento entre instancias) y T-F6-03 (degradación) |


## Bloqueos y solicitudes de decisión abiertas

Seis decisiones abiertas (DR-20261031-01/02/03 aceptadas el 2026-10-31). Todas son contradicciones o información faltante reales
entre documentos normativos; `AGENTS.md` prohíbe que un agente las resuelva por su
cuenta. Los **kernels puros** de todas ellas están implementados y probados; lo
que falta es el esquema o la decisión de contrato.

| ID | Tarea | Qué bloquea | Qué **no** está bloqueado |
|---|---|---|---|
| DR-20261017-01 | T-E3-02 | Publicación por cambio significativo: `22` §3.3 (cinco condiciones, umbral `max(min_distance, factor·accuracy)`) contradice `sampling_policy.json` (`SMP-PUB-002` publica por debajo del umbral; `SMP-PUB-003` usa una razón `accuracy` inexistente). | Selección de intervalo (`SMP-INT-001..005`) y cola offline (`SMP-OFF`) implementadas y con vectores. |
| DR-20261018-01 | T-E6-06 | Formato `gsp1` sin especificar en ningún documento, y ruta de traza divergente: OpenAPI `/sessions/{id}/trace` vs `24` §4/`03` `/sessions/{id}/members/{member_id}/trace`. | `GET /sessions/{id}/trace` implementado en la ruta del OpenAPI con `json`/`polyline` y `gsp1` según la propuesta del DR. |
| DR-20261018-02 | T-E8-04/05 | `edge_reports.finder_device_id NOT NULL` (`11` §2.0) contradice T-28 y `25` §2.4 («no guardar el dispositivo del buscador junto al blob»). | Almacén de tags (`/v1/tags`) y kernels de E8 (HMAC, EID, DULT) listos. |
| DR-20261018-03 | T-E7-01..04 | `11` §2.0 **no define ninguna migración** para la capa aprendida (perfiles de velocidad, ETA, sinuosidad, lugares, demanda/oferta, matrices OD), que `24` §3.1 y `07` §9.11 sí requieren. | Kernels de E7 (DBSCAN, eventos de conducción, cubetas de velocidad, sinuosidad, k-anonimato) implementados. |
| DR-20261018-04 | T-E8-08 | `edge_lpwan_integrations.secret_hash` (`11` §2.0) no puede verificar un HMAC, que necesita la clave; `07` §10.3 guarda el secreto cifrado. Ruta del webhook divergente (OpenAPI vs `25` §4). | Trait `LpwanDecoder` con registro y decodificador de referencia listos. |
| DR-20261031-04 | T-E5-07, T-E7-05/06 | Leer `.osm.pbf` necesita descomprimir zlib (sin dependencia aprobada) y `17` no define los parámetros `mm_*` de `24` §3.3. | Grafo vial y map-matching HMM implementados y probados con red sintética. |

**Bloqueos operativos (no son decisiones de diseño), detectados el 2026-10-31:**

| Bloqueo | Efecto | Quién lo resuelve |
|---|---|---|
| GitHub Actions no arranca ningún job: «the job was not started because your account is locked due to a billing issue». Los ocho últimos runs de `main` y de las ramas fallaron por esto en 2 s, no por el código. | Ningún PR puede cumplir la DoD de `19` §1 («pasan localmente **y en CI**»). La verificación queda en local hasta que vuelva CI. | Propietario: facturación de la cuenta de GitHub |
| `cargo deny check advisories` falla también en `main`: RUSTSEC-2026-0381, -0382, -0383, -0385 y -0386 sobre dependencias transitivas (raíces de certificados, `rustls-webpki` y otras). | Etapa de seguridad roja con independencia de cada PR. | PR dedicado de actualización de dependencias (`19` §6.1) |

Las cuatro decisiones que bloqueaban F4 están resueltas.

| ID | Tarea | Estado | Decisión | Fecha |
|---|---|---|---|---|
| DR-20261012-02 | T-F2-02 | **aceptada** | Opción 1: las invariantes que hace cumplir el esquema PostgreSQL basta con probarlas en `gs-store` contra una base real, además de la prueba de dominio. Enmendado en `05` §11 | 2026-10-15 |
| DR-20261012-03 | T-F2-03 | **aceptada** | Opción 1: resolución de tenant desde la entidad, con mutaciones acotadas al tenant resuelto | 2026-10-15 |
| DR-20261012-04 | T-F2-06 | **aceptada** | Opción 1: `geostream` puede depender de `gs-observability`; `tools/src/crate_deps.rs` actualizado | 2026-10-15 |
| DR-20261013-01 | T-F4-02 | **aceptada** | Opción 1: `geostream` → `gs-config` permitido, y `serve` monta `/v1` y `/v1/stream` según los roles activos | 2026-10-15 |

## Fases

| Fase | Estado | Cierre | Evidencia |
|---|---|---|---|
| F0 | completada | 2026-10-12 | CI verde (etapas 1–5), geostream version, gs-protocol roundtrip, geostream-tools (Rust puro), deploy dev, nombres verificados |
| F1 | completada | 2026-10-12 | Vectores de dominio verdes (ids, location, invitation, stream), invariantes INV-* con 19 pruebas nombradas, gs-domain puro sin I/O, property tests con proptest, cobertura de líneas 90.76 % (>= 85 % NFR-MAINT-002) |
| F2 | completada | 2026-10-12 | Tokens emitidos y verificados con los casos negativos de `06` §4.3 (paso 4 incluido: tenant/aplicación desactivados, época de subject y dispositivo desactivado), migraciones idempotentes, `/health/*` y `/metrics` operativos en el listener de administración, verificador de captura de logs activo, TLS de referencia en `deploy/` |
| F3 | completada | 2026-10-13 | Todas las operaciones con `x-gs-phase` ≤ F3 implementadas y conformes: suite de conformidad OpenAPI con matriz de autorización REST completa y aislamiento de tenants (T-F3-11) |
| **F4 (M1)** | **completada** | 2026-10-15 | T-F4-01 a T-F4-13 cerradas. SCN-001..010 verdes en ambos subprotocolos; plazo de revocación medido dentro de NFR-SEC-002; `gs-devclient` con demo M1 (dos publicadores y un lector); suite de privacidad T-F4-13 verde (3/3). Release `server-v0.1.0-alpha.1` |
| F5 (M2) | pendiente | | El SDK Flutter pasa a ser uno más: la lógica de cliente vive en `gs-client-core` (ADR-0022 sustituye a ADR-0012) |
| E0 | **completada** | 2026-10-15 | Adopción documental del addendum A-02: `docs/22`–`docs/26`, ADR-0021..0032, enmiendas de `docs/00`–`docs/21`, contrato ampliado, vectores `TRK/SMP/POOL/ETA/PRD/ASG/EDGE` y escenarios SCN-014..026. `geostream-tools all` con 9 comprobaciones en verde |
| E1 | **completada** (T-E1-01..T-E1-06) | | Ver detalle arriba | 
| E2 | **en curso** (núcleo) | | gs-track añadido: Kalman CV, plausibilidad, odometer, stops. Integración storage/webhooks en E3+ |
| E2 | pendiente | | |
| E3 | pendiente | | |
| E4 | pendiente | | |
| E5 | pendiente | | |
| E6 | pendiente | | **Bloqueada por T-E6-11**: revisión legal y DPIA antes de cualquier release |
| E7 | pendiente | | |
| E8 | pendiente | | **Bloqueada por T-E8-06**: protección DULT completa antes de cualquier release |
| E9 | pendiente | | |
| F15 | **en curso** (activada 2026-10-31, DR-20261031-05) | | T-F15-01 completada: adopción documental de la consola de desarrolladores |
| F6 | **en curso** (adelantada al cerrar F4) | | T-F6-01: backends Redis/Valkey con la suite compartida verde y `serve` usándolos |
| F7 (M3) | pendiente | | |
| F8 | pendiente | | |
| F9 (M4) | pendiente | | |
| F10 | pendiente | | |
| F11 | pendiente | | |
| F12 (M5) | pendiente | | |

## Registro

Formato de cada entrada (más reciente arriba):

### 2026-11-01 — T-F15-09 — Portal público: landing, documentación publicada y estado
- Autor/agente: Angel Nereira (con agente de código).
- Resultado: completada en local (imagen construida y probada); **sin desplegar**: necesita la rama en `main`, un servicio nuevo en Railway y un registro DNS que crea el propietario. Cierre formal pendiente de CI (facturación de GitHub).
- Origen: DR-20261101-01 y ADR-0034. El propietario pidió un portal con landing, contratos publicados, disponibilidad, acceso y mapas en tiempo real; esta parte es lo que no depende de nada externo. El acceso con API key exige CORS (T-F15-11) y el mapa propio (T-F15-10); el inicio de sesión con Gravital sigue bloqueado por OIDC (T-F15-02).
- Qué: `apps/portal` (Flutter Web, solo `http` de la lista aprobada), `tool/build_docs.mjs`, `nginx.conf.template`, `Dockerfile` y `Dockerfile.dockerignore` propio; `geostream-tools portal-docs`; etapa 6 de CI.
- Defectos encontrados al probar: (1) nginx descartaba las cabeceras de seguridad de `/` porque un `location` con su propio `add_header` reemplaza las del nivel `server`; ahora cada `location` incluye `security-headers.conf`. (2) La referencia de la API que genera Redocly carga su script y fuentes desde CDN de terceros y usa estilos en línea, incompatible con una CSP de `script-src 'self'`; se aloja Redoc 2.5.1 en el propio portal. (3) El `.dockerignore` de la raíz excluye `docs/` y `*.md`; el portal usa `apps/portal/Dockerfile.dockerignore`.
- Verificación: `flutter analyze` sin avisos; `flutter test` 10/10; `cargo test -p geostream-tools` 17/17 (5 nuevas); `cargo clippy --workspace --all-targets --all-features -- -D warnings` y `cargo fmt --check` OK; `docker build` desde cero OK; contenedor en marcha: `/`, `/docs/api.html`, guías, `.proto` y OpenAPI responden 200 y `10`/`16` 404; Chrome headless carga landing, guía y referencia de la API sin violaciones de CSP (el único bloqueo es un logo decorativo de Redoc en su CDN). Imagen de ~42 MB de contenido estático.
- Pendiente de decisión del propietario: dominio (`console.geostream.gravitalcloud.com`, registro CNAME que da Railway), y crear el servicio en Railway tras fusionar.
- Nota: contra la API en producción el estado muestra «Sin datos» porque aún devuelve el cuerpo anterior de `/health/ready`; se resuelve al desplegar T-F6-03.

### 2026-10-31 — T-F15-01 — Adopción de la consola de desarrolladores (sin código)
- Autor/agente: Angel Nereira (con agente de código).
- Resultado: completada.
- Origen: el propietario pide un portal web donde cualquier desarrollador entre con su cuenta de Gravital, obtenga API keys y use GeoStream como servicio, al estilo de Google Maps Platform. DR-20261031-01 había dejado esa consola «solo si GeoStream se ofrece a terceros»; ahora es el caso. Respuestas del propietario en DR-20261031-05.
- Hallazgo que condiciona el plan: `identity` de Gravital (Plenty-Pay) firma con secretos simétricos, no publica JWKS ni OIDC y no tiene dominio público (Railway, 2026-10-31). La consola no puede abrirse hasta T-F15-02, que se hace en ese repositorio.
- Qué: ADR-0033; FR-ADM-004 activado y FR-CON-001 a FR-CON-008; amenazas T-32 a T-37; `17` §2.12 y cuotas de §4; fase F15 en `21`.
- Verificación: `cargo run -p geostream-tools -- all` (resultado en el PR).
- Efecto en producción: ninguno (solo documentación). Despliegue de T-F6-01 comprobado en Railway: `SUCCESS`, log `state backend: redis` y `/health/ready` 200 en `geostream.gravitalcloud.com`.

### 2026-10-31 — T-F6-01 (parte 2) — `serve` usa el backend configurado
- Autor/agente: Angel Nereira (con agente de código).
- Resultado: hecha en local; el cierre formal espera a que CI vuelva a ejecutar.
- Qué: `EphemeralBackend` y `BusBackend` (enums que despachan a memoria o a Redis; los traits usan `async fn` y no admiten `dyn`, y con un tipo concreto los futures siguen siendo `Send` para axum) sustituyen a `InMemoryEphemeralStore`/`InMemoryBus` en `gs-api`, `gs-stream` y `gs-testkit`. `serve` elige con `GS_STATE_BACKEND`; con `redis` conecta antes de abrir el listener y no arranca si no alcanza `GS_REDIS_URL`; con `memory` en `prod` avisa en el log (`17` §3).
- Verificación: `cargo clippy --workspace --all-targets --all-features -- -D warnings` OK; `cargo fmt --all -- --check` OK; `cargo test --workspace --all-features` 675/675. `geostream serve` con `GS_STATE_BACKEND=redis` contra Valkey 7.2: `/health/ready` 200, el log dice `state backend: redis` y Valkey muestra dos clientes (RESP2 del almacén y RESP3 del bus). Con `GS_REDIS_URL` inalcanzable sale con código 1 y el log no contiene la contraseña de la URL.
- **Efecto en producción al fusionar:** el servicio `geostream` de Railway ya tiene `GS_STATE_BACKEND=redis`, así que empezará a usar el Redis gestionado. Pendiente del propietario: revisar que ese Redis cumpla `11` §3 (sin persistencia, `noeviction`).
- Pendiente: los escenarios SCN-001..008 con publicador y suscriptor en instancias distintas son T-F6-06; `/health/ready` todavía no informa del almacén efímero ni del bus, y la degradación de `04` §7.6 es T-F6-03.

### 2026-10-31 — T-F6-01 (parte 1) — `gs-ephemeral` y `gs-bus` sobre Redis/Valkey
- Autor/agente: Angel Nereira (con agente de código).
- Resultado: parte 1 completada (backends y suites). La tarea sigue **en curso**: falta el cableado en `serve`.
- Por qué ahora: F6 es adelantable desde el cierre de F4 (`21` §F6) y el despliegue de Railway ya define `GS_STATE_BACKEND=redis` con un Redis gestionado. Sin embargo, `apps/geostream/src/main.rs` crea siempre `InMemoryEphemeralStore` e `InMemoryBus`, así que ese Redis no se usa y el servicio no puede tener más de una instancia.
- Requisitos: FR-STR-009 y FR-PRS-003 pasan a `en curso` (la aceptación completa exige el servidor cableado y los escenarios multi-nodo de T-F6-06).
- Qué: `RedisEphemeralStore` con el keyspace de `11` §3, scripts Lua en `crates/gs-ephemeral/lua/` (secuencias u64 comparadas como cadenas decimales, porque los números de Lua son `double` y pierden precisión por encima de 2^53), `MULTI` para el resto, TTL en todas las claves y timeout por comando. `RedisBus` con una conexión RESP3 por instancia, refcount por tema, colas acotadas y resuscripción automática.
- Hallazgos: `11` §3 llamaba `track_update.lua` al script que `03`, `22` y ADR-0023 llaman `track_cas.lua`; se corrige `11`, que es de menor rango (`00` §3). Había un borrador sin uso en `crates/gs-ephemeral/scripts/track_cas.lua` que guardaba un string `"<version>\n<blob>"` y afirmaba que un hash de Redis no admite binarios (sí los admite); se retira. El valor de `Bus::publish` no puede significar lo mismo en los dos backends (Redis cuenta conexiones, es decir, instancias), así que la suite comprueba que todos los suscriptores reciben, no el número.
- Verificación (local, Valkey 7.2 de `deploy/docker-compose.dev.yml` y PostgreSQL local): `cargo fmt --all -- --check` OK; `cargo clippy --workspace --all-targets --all-features -- -D warnings` OK; `cargo test --workspace --all-features`: 675 pruebas, 0 fallos; `cargo test -p gs-ephemeral` 11/11 Redis y `cargo test -p gs-bus` 6/6 Redis; `cargo run -p geostream-tools -- all` OK. Comprobado a mano: tras `docker restart` del servidor el bus se resuscribe y entrega; con el servidor parado `is_connected()` es `false` y `publish` devuelve error, y se recupera solo. **No verificado en CI** (cuenta bloqueada, ver bloqueos). `cargo deny check advisories` falla igual que en `main`.

### 2026-10-31 — Precisión estricta, rechazo opcional de ubicaciones simuladas y núcleo de map-matching sobre OSM
- Autor/agente: Angel Nereira (con agente de código).
- Resultado: completada la parte no bloqueada; el lector `.osm.pbf` y los parámetros `mm_*` quedan en DR-20261031-04.
- Requisitos: FR-LRN-003 y FR-HIS-004 pasan a `en curso`. Decisiones DR-20261031-01/02/03 aceptadas por el propietario («Dale procede con todo»).
- Qué: reglas 17–18 de `09` §3.1 y claves `reject_mock_samples`/`reject_network_only`; `max_horizontal_accuracy_m` desde 5 m; `gs-mobility::road_graph` y `gs-mobility::map_matching`.
- Verificación: `cargo clippy --workspace --all-targets -D warnings` y `cargo fmt --check` en verde; `cargo test --workspace` con PostgreSQL y Redis locales: 516 pruebas en verde y 2 fallos de `gs-store/tests/history_test.rs` que solo aparecen con toda la suite en paralelo sobre la misma base (pasan 3/3 en aislamiento, con y sin este cambio; no tocan el código modificado); `gs-mobility` 9/9; `geostream-tools all` en verde; `flutter analyze` y `flutter test` 37/37.

### 2026-10-31 — Guía de consumo del despliegue y tres solicitudes de decisión
- Autor/agente: Angel Nereira (con agente de código).
- Resultado: documentación completada; la implementación de cuentas, antifraude y precisión estricta queda **bloqueada por decisión**.
- Qué: `15` §10 explica cómo un proyecto consume `geostream.gravitalcloud.com` con lo ya implementado (alta por CLI, intercambio de token con la identidad propia de cada proyecto, transporte para móvil, microcontrolador y gateway, campos de precisión, y qué controles de precisión y plausibilidad existen). DR-20261031-01/02/03 recogen lo que pide el propietario y choca con `06` §2, D-10 y `17` §4.
- Verificación: `cargo run -p geostream-tools -- all` en verde. Cada afirmación de `15` §10 contrastada con el código o el contrato (`gs-service/src/publishing.rs`, `location.proto`, `09` §3, `17` §4).

### 2026-10-31 — Encargo del propietario — app Flutter de pruebas con mapa OSM y editor de geocercas; dominio de producción
- Autor/agente: Angel Nereira (con agente de código).
- Resultado: completada (fuera del orden del roadmap, por petición directa del propietario).
- Requisitos: ninguno se marca cumplido. Toca FR-SDK-013 (app de ejemplo, F5), FR-MAP-003 (MapLibre, F9) y FR-GEO-001/002 (F11) solo como herramienta de prueba: no sustituye al SDK ni al servidor de geocercas.
- Qué: `apps/flutter_app` publica muestras reales por la ingesta HTTP y dibuja el snapshot sobre OpenStreetMap; el editor guarda geocercas en el teléfono, las valida con `14` §3 y las evalúa en local con `14` §4. Servicio de Railway con dominio `geostream.gravitalcloud.com` y despliegue desde `main`.
- Verificación: `flutter analyze` sin avisos; `flutter test` 37/37; `cargo run -p geostream-tools -- all` en verde; `cargo build -p geostream` en verde tras excluir la app del workspace; despliegue de Railway desde `main` en `SUCCESS` con migraciones a la versión 5. **Pendiente del propietario**: crear los registros DNS (`deploy/README.md` §Dominio público) para que Railway emita el certificado; y compilar el APK/IPA con el SDK de Android o Xcode, que no hay en el entorno del agente.
- Pendiente para el roadmap: cuando exista `POST /v1/geofences` (F11), la app puede enviar el JSON que ya genera.

### 2026-10-15 — Defecto corregido — el close frame podía adelantar al `StreamError` que lo acompaña
- Autor/agente: Angel Nereira (con agente de código).
- Resultado: corregido.
- Requisitos: FR-AUTH-007 (`08` §6.6, §8).
- Hallazgo: la prueba `test_fr_auth_007_auth_refresh_with_another_device_token_fails_permission_denied_and_closes_4403` fallaba de forma intermitente (≈ 1 de cada 4 ejecuciones). No era una prueba inestable: era un **defecto de producción**. El `select!` del escritor de la conexión (`crates/gs-stream/src/connection.rs`) no era `biased`, así que cuando la cola de control tenía un `StreamError` pendiente **y** el close frame ya estaba en su canal, Rust elegía la rama **al azar**. Si elegía la del cierre, el `StreamError` se quedaba sin escribir y el cliente recibía solo el 4403.
- Por qué importa: `08` §6.6 promete `StreamError{PERMISSION_DENIED}` y después cierre 4403. Un cliente que recibe únicamente el 4403 no puede distinguir «te rechacé el token» de «la sesión expiró», que es justo lo que necesita para decidir si reintenta. La misma pérdida podía afectar a cualquier mensaje de control seguido de un cierre.
- Corrección: `select!` pasa a `biased;` con la rama de control primera, y la rama de cierre **drena la cola de control antes de escribir el close frame**. Documentado en `08` §8 («el close frame va siempre después») y en `06` §8.3.
- Verificación: ocho ejecuciones consecutivas de `cargo test -p gs-stream --test connection_test` en verde (50/50 cada una), frente a 1 fallo en 3 de 4 ejecuciones antes de la corrección. Además se corrigió la causa de la otra prueba intermitente del mismo fichero: `test_fr_auth_007_connection_closes_with_4401_if_auth_expires_without_refresh` competía con un TTL de token de **1 segundo**, de modo que bajo carga el cierre podía llegar antes de la respuesta al hello; ahora usa 3 s y `read_binary` explica con claridad que un cierre no es un frame inesperado.

### 2026-10-15 — T-E1-01 — `gs-geo`: geodesia pura
- Autor/agente: Angel Nereira (con agente de código).
- Resultado: completada.
- Requisitos: — (habilitante de FR-CORE-001, FR-TRK-001, FR-MOB-001, FR-HIS-004).
- Evidencia: 35 pruebas, todas en verde. Sin dependencias de E/S, sin `tokio`, compatible con la regla de pureza de `19` §6.2.2 y con la compilación a WASM que exige `26` §1.2.
- **Dos distancias, y cada una dice cuál es.** `haversine_distance` (esfera de radio medio IUGG) y `geodesic_distance` (Vincenty sobre WGS84) devuelven esta última un `DistanceMethod`. Cerca del antípoda, donde Vincenty no converge, se cae a la esfera y **se declara** `HaversineFallback` en vez de devolver un número elipsoidal falso: un caller que necesita precisión elipsoidal tiene que poder detectar que no la tuvo.
- **La diferencia entre modelos se midió antes de escribir la prueba.** Resultado: llega al 0,56 % y depende de la **dirección**, no de la escala. En el ecuador un tramo norte-sur mide 6 335 439 m en el elipsoide frente a 6 371 009 m en la esfera (ratio `M/R = a(1−f)²/R`), y un tramo este-oeste al revés (ratio `a/R`). Mi primera versión de la prueba afirmaba «a corta distancia coinciden»; era falsa y la replaced por dos aserciones contra los parámetros del elipsoide, que son más fuertes: fallan si alguien cambia el elipsoide, el radio medio o intercambia los términos de latitud y longitud.
- **Un bug real en `destination`**: usaba `sin θ` donde la fórmula de círculo máximo necesita `cos θ`. El resultado era plausible y estaba en la dirección equivocada, que es la clase de error que una prueba de humo no ve. Ahora coincide bit a bit con `geo`, y hay una prueba de ida y vuelta rumbo/distancia a cuatro pares.
- **La simplificación trabaja en metros.** `geo::Simplify` usa las unidades de la geometría, y las nuestras son grados; un grado de longitud no es una distancia y se encoge hacia los polos. Se proyecta al marco local, se simplifica en metros y se vuelve a proyectar. Comprobado con un desvío de 10 m a 64° N: sobrevive a 3 m de tolerancia y desaparece con 50 m.
- **El sesgo del marco local queda documentado en el tipo.** `LocalFrame` usa el radio medio en ambos ejes para ser coherente con `haversine_distance`; a cambio, sobre el ecuador su escala norte-sur excede en un 0,56 % la del arco meridiano WGS84. Mezclar un marco esférico con una distancia elipsoidal convertiría un sesgo constante en una función de la dirección, que es peor.
- Corregido de paso un defecto de `docs/17` §2.11: declaraba **dos valores distintos bajo el nombre `EARTH_MEAN_RADIUS_M`** (6 371 008,8 y 6 371 000,0). Ahora hay tres constantes con nombre propio: `EARTH_MEAN_RADIUS_M` (esfera), `EARTH_SEMIMAJOR_AXIS_M` y `EARTH_FLATTENING` (elipsoide), con la explicación de por qué son cosas distintas.
- Comandos: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo test --workspace` (503 pruebas, 0 fallos), `cargo run -p geostream-tools -- all` (10/10 OK).
### 2026-10-15 — T-E1-03/T-E1-04 — Runtime + FFI
- `gs-client-runtime`: transport shell (tokio WS + reqwest) con una prueba de humo sobre eco local. El acoplamiento con el protocolo wire y las pruebas contra el servidor real quedan para E2/E3 (el runtime ya expone `Runtime`, `TransportConfig`, `send`).
- `gs-ffi`: handles opacos, `catch_unwind`, `#[unsafe(no_mangle)]` con `# Safety` y `// SAFETY:` en cada bloque, el único crate del workspace con `unsafe`. 4 pruebas de creación/destrucción y de robustez ante `NULL`.
- `tools/src/crate_deps`: se declara que `gs-ffi` depende directamente de `gs-client-core` además del runtime, porque la FFI llama al núcleo sin pasar por I/O.
- Siguiente: T-E1-05 ya cubierto en el workflow de CI (musl + wasm32), T-E1-06 cubierto por `crates/gs-client-core/tests/properties.rs`.

### 2026-10-17 — Pools rings/density REST y checkpoint del filtro
- Autor/agente: Angel Nereira (con agente de código).
- Resultado: completada.
- Evidencia:
  - `GET /v1/pools/{id}/rings` (paso o `k`, exactamente uno) y `/density` (accesible a dispositivo con `pools:density`) montados y verificados en vivo (`401` sin credencial).
  - T-E2-06: `TrackStateMachine::encode_checkpoint`/`decode_checkpoint` con ida y vuelta exacta (misma estimación tras restaurar) y rechazo de blob malformado; guardado en `{p}:trk:{device}` tras cada publicación aceptada y restaurado en el hello.
- Pendiente: `availability`/`attributes` reales (T-E4-05); TTL del checkpoint desde `GS_TRACK_STATE_TTL_S` (hoy constante documentada); prueba de integración 200 con política habilitada.

### 2026-10-17 — E2 en la ingesta y REST de pools
- Autor/agente: Angel Nereira (con agente de código).
- Resultado: completada (E2 en ingesta + `nearby`).
- Evidencia:
  - `publish_batch` aplica `gs-track` a cada muestra aceptada y adjunta `LocationUpdate.estimate` (FILTERED); la muestra `MEASURED` viaja intacta. Prueba `test_fr_trk_001_estimate_is_attached_beside_the_sample`.
  - `GET /v1/pools/{session_id}/nearby` montado; verificado en vivo (401 sin credencial). Lee el índice de los snapshots y aplica `gs-pools::nearby`.
- Defecto corregido: el verificador de privacidad marcaba la fracción de un timestamp ISO-8601 (`…18.982593`) como coordenada (`8.982`), haciendo fallar la prueba según el reloj. Ahora exige que el patrón no vaya precedido de un dígito.
- Pendiente: `rings`/`density` REST, `availability`/`attributes` reales (T-E4-05), checkpoint del filtro en `{p}:trk:{device}` (T-E2-06) y prueba de integración del camino 200 con política habilitada.

### 2026-10-17 — E4: consultas de pool (núcleo)
- Autor/agente: Angel Nereira (con agente de código).
- Resultado: parcial (núcleo de consultas).
- Evidencia: `gs-pools` implementa `nearby`, `rings` y `density` con sus vectores (`POOL-NEAR-001..005`, `POOL-RING-001/002`, `POOL-DENS-*`): exclusión de `PAUSED` y desconectados, filtro por atributos, orden por distancia o ETA, anillos con `truncated`, centroide desplazado y rumbo cuantizado, tope de marcadores. 10 pruebas.
- Nota corregida: la ingesta de ubicación **sí** existe (`publish_batch` escribe snapshots). Lo que faltaba era el motor E2, ya cableado; y el índice de pool (leer snapshots y proyectar disponibilidad/atributos) para los endpoints REST.
- Siguiente: ingesta de ubicación en `gs-stream` (motor E2 + índice E4) → luego los endpoints REST de pools.

### 2026-10-17 — REST de la pista E: herramientas de movilidad (E5)
- Autor/agente: Angel Nereira (con agente de código).
- Resultado: parcial (endpoints de herramientas).
- Evidencia: `POST /v1/tools/eta` y `POST /v1/tools/assignments` montados en `serve` (rol `api`); verificados en vivo contra el binario y la base de test: `401 UNAUTHENTICATED` sin credencial y con credencial malformada, con Problem Details y `request_id`. 31 pruebas de `gs-api`.
- Decisiones: se añaden al `ErrorCode` de Rust los tres códigos de la pista E que faltaban (`FEATURE_DISABLED`, `CONSENT_REQUIRED`, `PROVIDER_UNAVAILABLE`) y a la política del tenant `tools_allowed`/`pools_allowed`/`edge_allowed`, todos por defecto `false`.
- Pendiente: `min_cost_flow` con capacidad usa una asignación voraz con capacidad (el solver MCF es T-E5-02 dedicado); prueba de integración con API key real y política habilitada.

### 2026-10-17 — Migraciones de la pista E (E2/E6/E8)
- Autor/agente: Angel Nereira (con agente de código).
- Resultado: completada (esquema).
- Evidencia: `0003_engine_state.sql`, `0004_history.sql`, `0005_edge.sql` aplicadas contra PostgreSQL 16 real; `location_points` es particionada (`relkind='p'`); `history_mode` por defecto `OFF`; `consent_purposes` por defecto `{}`; `tags` sin columna `EIK`. Pruebas de invariantes de esquema en `gs-store` (contra base) y guardas puras de migraciones incrustadas.
- Hallazgo: `sqlx::migrate!` no recompila al añadir un fichero nuevo; documentado en `docs/11` §2.0. La imagen Docker compila desde cero y no se ve afectada.
- Siguiente: handlers REST/WS reales en `gs-api`/`gs-stream` (hoy solo `/health/*`).

### 2026-10-15 — T-E1-02 — `gs-client-core`: núcleo sans-IO

- Autor/agente: Angel Nereira (con agente de código).
- Resultado: completada.
- Requisitos: habilitante de FR-CORE-001 (se cierra en E3 con el runtime).
- Evidencia: 11 pruebas propias; `cargo build -p gs-client-core --target wasm32-unknown-unknown` OK; `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo test --workspace` (514 pruebas, 0 fallos), `cargo deny check`, `cargo run -p geostream-tools -- all` (10/10 OK).
- Decisiones:
  - `gs-geo` pierde la dependencia de `gs-domain` (solo eran comentarios): desbloquea el wasm.
  - Umbrales de muestreo se inyectan, nunca se clavan a constantes del SDK.
  - `#[allow(hidden_glob_reexports)]` acotado a las importaciones privadas que shadow la glob re-exportación.
- Siguiente: T-E1-03 (`gs-client-runtime`, desbloquea el `gs-devclient` real).

### 2026-10-15 — T-E0-08 (ampliación) — Verificador de enums y corrección de `docs/22` §2.2
- Autor/agente: Angel Nereira (con agente de código).
- Resultado: completada.
- Evidencia: al revisar `docs/22` §2.2 contra el contrato antes de abrir E1 se encontró una **contradicción real**: la tabla de banderas de plausibilidad usaba `IMPLAUSIBLE_SPEED`, `IMPLAUSIBLE_ACCELERATION`, `MOCK_REPORTED`, `FILTER_OUTLIER` y `TIME_GAP`, mientras que `location.proto` declara `EXCESSIVE_SPEED`, `IMPLAUSIBLE_ACCEL`, `MOCK_FLAGGED`, `OUTLIER_JUMP` y ninguna `TIME_GAP`. Cuatro documentos contra el contrato, y nada en CI lo detectaba.
- Corrección: se alinea `docs/22` con el contrato (tres artefactos coincidían: proto, OpenAPI y vectores) y se documenta **por qué** `TIME_GAP` no es una bandera: un hueco temporal es un motivo de reinicio del filtro (`TrackResetReason`: `GAP`, `OUTLIER_RUN`, `FRAME_REANCHOR`, `FIRST_FIX`), no una señal de plausibilidad. La razón es de comportamiento interno y no viaja en el contrato; las banderas sí, porque el integrador las necesita (D-10). Se corrigen también las dos referencias residuales a `LocationKind` donde el contrato define un `EstimateKind` propio, y se tabulan las penalizaciones junto a cada bandera en lugar de en un párrafo aparte.
- Causa raíz: **no existía ningún control de enums**. `geostream-tools` comprobaba códigos de error, scopes, fases, prefijos de ID, claves de configuración e IDs de requisito, pero nunca que un valor de enum documentado exista en el contrato. Añadido `tools/src/enums.rs` (subcomando `geostream-tools enums`, incluido en `all`, 12 pruebas unitarias), que verifica proto ↔ OpenAPI para los 8 enums con schema REST y docs → contrato para las tablas declaradas, en las dos direcciones: un nombre que no existe y un valor que nadie documentó son ambos fallos.
- El verificador encontró un segundo defecto **en el momento de activarlo**: `ProviderPort` era `SCREAMING_SNAKE` en el proto y minúsculas en el OpenAPI, los vectores y los escenarios. Corregido en los cuatro sitios, y con ello se fija en `contracts/README.md` §3 la convención de nombres de enum, que antes solo estaba implícita.
- Comandos: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo test --workspace` (468 pruebas, 0 fallos), `cargo run -p geostream-tools -- all` (10/10 OK).
### 2026-10-15 — T-E0-08 — Verificador de fases, scopes y prefijos de ID (`geostream-tools`)
- Autor/agente: Angel Nereira (con agente de código).
- Resultado: completada.
- Requisitos: NFR-DOC-001.
- Evidencia: nuevo módulo `tools/src/phases.rs` con subcomando `geostream-tools phases`, registrado también en `all`. Comprueba tres cosas que hasta ahora nadie miraba y que una revisión de PR no detecta: (1) todo `x-gs-phase` del OpenAPI corresponde a una fase declarada en `docs/21_ROADMAP.md` —un `E4` en una operación de E5 no rompe nada visible, solo rompe la regla de que una capacidad nueva no se activa por accidente—; (2) todo scope usado en `x-gs-auth` está documentado en `docs/06_IDENTITY_AUTH.md` §3.4 o §6, de modo que un scope que solo existe en el contrato se detecta; (3) los prefijos de ID de `docs/05_DOMAIN_MODEL.md` §2 coinciden con `contracts/test-vectors/ids.json` y con los `pattern:` del OpenAPI, con filtro estricto de la forma pública `<prefijo>_<32 hex>` que descarta `external_subject_id`, `installation_id` y `StreamTicket`. 6 pruebas unitarias propias (`tools/src/phases.rs`). Resultado actual: 80 operaciones, 27 fases, 19 scopes de aplicación, 10 de dispositivo y 15 esquemas de ID. Comandos: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo test --workspace`, `cargo run -p geostream-tools -- all`: 9/9 OK.

### 2026-10-15 — T-E0-07 — Hoja de ruta de la pista E (`docs/21_ROADMAP.md` v0.3.0)
- Autor/agente: Angel Nereira (con agente de código).
- Resultado: completada.
- Evidencia: `docs/21_ROADMAP.md` sube a v0.3.0 e incorpora la pista E0–E9 con mapa de fases, 60 tareas `T-E<n>-<NN>`, dependencias y criterios de salida por fase. Los criterios de GA por versión (v1.0 línea base → v1.5) dejan explícito qué versión depende de qué revisión: E6 no se publica sin la revisión legal (T-E6-11) y E8 no se publica sin la protección DULT (T-E8-06). Re-alcance del resto del roadmap: F13 se elimina (era E3 + E6 + E2 + E7), F15 pasa a ser parte de E9 y conserva solo `FR-AUTH-009`, `FR-ADM-004` y `FR-INV-004`, F16 queda parcialmente satisfecha porque los tags Beacon de E8 ya tienen E2EE. Regla de orden añadida a `docs/20` §4: la adopción documental y la implementación no se mezclan en un PR, y solo un PR toca `contracts/` a la vez. Comandos: `cargo run -p geostream-tools -- requirements` OK tras corregir los IDs de la tabla de tareas a los realmente declarados en `03`.

### 2026-10-15 — T-E0-06 — Contrato de la pista E (único PR sobre `contracts/`)
- Autor/agente: Angel Nereira (con agente de código).
- Resultado: completada.
- Requisitos: NFR-COMPAT-003.
- Evidencia: todos los cambios de `contracts/` en un único PR, como exige la regla de orden de `docs/20` §4. **Protobuf** (todo aditivo): `ErrorCode` gana `FEATURE_DISABLED` (26), `CONSENT_REQUIRED` (27) y `PROVIDER_UNAVAILABLE` (28); `LocationUpdate.estimate` (campo 6, opcional, solo `FILTERED` o `MAP_MATCHED`); `TrackEvent` en `ServerMessage.body` (campo 28); `GEOFENCE_TRANSITION_DWELL`; `SESSION_MODE_POOL` (valor 4); `SessionInfo.history_mode` (campo 8); `MemberStatus.availability`/`attributes`/`purposes` (campos 9–11); `ServerHello.sampling` (campo 11); `ClientHello.core_version` (campo 8); `Subscribe.replay_trace` (campo 3); enums `EstimateKind`, `PlausibilityFlag`, `HistoryMode`, `Availability`, `ConsentPurpose`, `ProviderPort` y `SourcePreference`. **Ningún campo existente se renumera ni se elimina.** **OpenAPI**: 22 operaciones nuevas, 63 esquemas y 5 tipos de webhook, todas con `x-gs-phase`. **Vectores**: cinco ficheros nuevos (`TRK-*`, `SMP-*`, `POOL-*`, `ETA-*`/`PRD`/`ASG`/`ANA-*`, `EDGE-*`), cuyos valores sintéticos son también los que el verificador de redacción busca en logs y métricas. **Escenarios**: SCN-014..SCN-026. Comandos: `(cd contracts/proto && buf lint)` OK, `buf breaking --against '.../main...'` **sin errores**, `python3 -c 'yaml.safe_load(...)'` OK (55 paths, 148 esquemas, 9 webhooks), comprobación propia de que no queda ningún `$ref` colgante, `cargo run -p geostream-tools -- error-catalog` OK (28 códigos), `cargo run -p geostream-tools -- auth-matrix` OK (70 operaciones).

### 2026-10-15 — T-E0-05 — Enmiendas a `docs/02` y `docs/05`–`docs/20`
- Autor/agente: Angel Nereira (con agente de código).
- Resultado: completada.
- Evidencia: diecinueve documentos normativos amendados y cinco revisados a fondo. `docs/17_CONFIGURATION.md` recibe **todos** los números nuevos —112 claves `GS_*` y 82 de política, entre ellas §2.10 (motor), §2.11 (constantes no configurables) y §4.1 (política del motor)— porque es el único sitio donde viven; `geostream-tools config-keys` falla si alguno se cita fuera de ahí. `docs/10` incorpora las amenazas T-23 a T-31 y fija la regla que más pesa: una celda H3, una `LocationEstimate`, una polilínea `gsp1`, un centroide de parada, una posición de densidad o un perfil de velocidad de un individuo **son coordenadas a efectos de redacción**. `docs/19` añade el DoD de la pista E (§1.1), la excepción única de `unsafe` en `gs-ffi` (§6.2.1) y las reglas de pureza de los crates (§6.2.2). `docs/20` añade el orden de fases, el mapa de lectura por crate nuevo y cinco reglas anti-alucinación específicas, la más importante: *no afirmar precisión ni comportamiento de un proveedor, un hardware o una API móvil sin medirlo*. Comandos: `cargo run -p geostream-tools -- all` 9/9 OK tras iterar.

### 2026-10-15 — T-E0-01 a T-E0-04 — Visión, requisitos, ADR y documentos `22`–`26`
- Autor/agente: Angel Nereira (con agente de código).
- Resultado: completadas.
- Evidencia: `docs/01_VISION_SCOPE.md` v0.3.0 (motor de dos mitades, G-11..G-15, NG-4/5/6 reescritos, UC-09..UC-13, P-E1..P-E10, criterios de GA). `docs/03_REQUIREMENTS.md` con ~70 requisitos nuevos, `FR-MAP-005` enmendado y `FR-NAT-001` retirado; `TRACEABILITY.md` regenerado con 216 IDs. ADR-0021..0032 aceptados, con ADR-0005, 0012, 0014 y 0016 marcados como sustituidos, ADR-0013 ampliado y `docs/adr/README.md` regenerado. `docs/22`–`docs/26` creados como documentos normativos con el detalle técnico. `docs/00_INDEX.md` v0.3.0 con las filas 22–26 del mapa y la precedencia ampliada. El addendum A-02 pasa a `docs/proposals/` como material de origen archivado, y `docs/20` §9.4 dice que si el código o `22`–`26` lo contradicen, **`22`–`26` mandan**. Comandos: `cargo run -p geostream-tools -- requirements` OK (216 IDs).

### 2026-10-15 — DR-20261012-02, DR-20261012-03, DR-20261012-04 y DR-20261013-01 — Aprobadas
- Autor/agente: Angel Nereira (propietario).
- Resultado: las cuatro pasan de `docs/decisions/pending/` a `docs/decisions/accepted/` con **Opción 1** en todas y fecha 2026-10-15. Ninguna deja trabajo pendiente: las tres primeras ya estaban implementadas y solo faltaba la confirmación formal; la cuarta (DR-20261013-01) se resuelve con el cableado de `serve` ya hecho.

### 2026-10-15 — T-F4-13 — Verificación de privacidad de extremo a extremo (`gs-testkit`)
- Autor/agente: Angel Nereira (con agente de código).
- Resultado: completada.
- Requisitos: FR-PRV-004, NFR-PRIV-001, NFR-PRIV-002, NFR-SEC-003.
- Evidencia: `crates/gs-testkit/tests/privacy_test.rs` ejecuta SCN-001/002/003/006 en ambos subprotocolos con un suscriptor de `tracing` que captura todo, falla si aparece un valor prohibido, inspecciona las doce tablas reales del esquema PostgreSQL para confirmar que ninguna contiene coordenadas y comprueba el borrado **síncrono** del snapshot al detener, salir, revocar y cerrar. 3/3 en verde. Es la prueba que convierte `docs/10` §8.2 de una promesa en algo verificable.

### 2026-10-15 — T-F4-12 — `gs-devclient` (con wiring de `serve`)
- Autor/agente: Angel Nereira (con agente de código).
- Resultado: completada.
- Requisitos: FR-PRV-004 (parcial), NFR-PRIV-002.
- Evidencia: dos correcciones en un mismo commit porque las dos tasks estaban bloqueadas por las mismas decisiones. **Wiring de `serve`** (resuelve DR-20261013-01 y DR-20261012-04): `geostream serve` carga `ServerConfig::load_from_map`, inicializa `gs-observability` desde el entorno y monta `/v1` (rol api) y `/v1/stream` (rol stream); añade el rol opcional `analytics`; `tools/src/crate_deps.rs` acepta los crates de la pista E y fija las reglas de pureza y de no-dependencia servidor→cliente. **Demo M1**: corrige payloads (`MANY_TO_MANY` con `metadata.title`, `role: MEMBER`, objeto `capabilities`, `installation_id` UUID, `platform` en mayúsculas) y construye la URL del stream con `ws_base` en lugar de `stream_url`, que resolvía a `geostream.example.com`. **Redacción**: elimina las coordenadas de los logs; `publisher.rs` y `subscriber.rs` ya no registran la `url` con el ticket y las posiciones se muestran como `<redacted>`. Comandos: `cargo test -p geostream` 7/7, `cargo test -p gs-devclient` 3/3, `cargo test -p gs-testkit --test privacy_test` 3/3.

### 2026-10-15 — T-F4-11 — Runner de escenarios y suite e2e SCN-001..010 (`gs-testkit`)
- Autor/agente: Angel Nereira (con agente de código).
- Resultado: completada.
- Rama: `feat/T-F4-11-scenario-runner`.
- Requisitos: FR-PRV-001.
- Evidencia: implementación completa del runner de escenarios y cliente de prueba en `crates/gs-testkit` (`docs/18_TESTING_QA.md` §6, `contracts/README.md` §5). Soporta ejecución declarativa del DSL de escenarios (`contracts/scenarios/*.json`) con aprovisionamiento dinámico de servidores Axum en puertos efímeros (`0`), negociación de subprotocolos (`gs.v1.json` y `gs.v1.proto`), sustitución recursiva de plantillas (`{{var}}`, `{{var.path}}`), evaluación de expresiones JSONPath (`json_path`), y comparación parcial y exacta de mensajes (`matcher`) adaptada a la omisión proto3 de valores por defecto. Se implementó la suite completa de pruebas para los escenarios SCN-001 a SCN-010 bajo ambos subprotocolos (20 pruebas en `crates/gs-testkit/tests/scenarios_test.rs`), incluyendo SCN-009 bajo `#[ignore = "slow/nightly"]` (verificado verde en ~59s). Pruebas: 18 pruebas automáticas por defecto + 2 nocturnas pasando. Comandos: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo test --workspace` (451 pruebas pasando, 0 fallos; base en `main`: 421 → +30), `cargo deny check`, `cargo run -p geostream-tools -- all`: todos OK.

### 2026-10-15 — T-F4-10 — Snapshot REST (`GET /v1/sessions/{id}/snapshot`)
- Autor/agente: Angel Nereira (con agente de código).
- Resultado: completada.
- Rama: `feat/T-F4-10-session-snapshot`.
- Requisitos: FR-INT-002.
- Evidencia: implementación completa del endpoint `GET /v1/sessions/{id}/snapshot` según `docs/07_REST_API.md` §9.6, `docs/03_REQUIREMENTS.md` (FR-INT-002) y OpenAPI 3.1 (`getSnapshot`). Permite consultar las últimas ubicaciones retenidas de los miembros activos que tienen la compartición encendida (`sharing=true`). Autorización estricta: aplicaciones requieren el scope `locations:read` (403 `INSUFFICIENT_SCOPE`), y dispositivos requieren ser miembros activos de la sesión con la capacidad `read` habilitada (404 `NOT_FOUND` para no miembros y 403 `PERMISSION_DENIED` para miembros sin `read`). Si la sesión está cerrada responde 409 `SESSION_CLOSED`; si está expirada o vencida responde 409 `SESSION_EXPIRED`. Si la sesión tiene `snapshot_ttl_seconds = 0`, la retención de snapshots está desactivada y devuelve la lista de items vacía (`[]`). Para cada miembro con compartición activa que cuenta con snapshot en el almacén efímero, se proyecta su última muestra con `LocationSample` validado y conforme a OpenAPI, `received_at` en RFC 3339 y `age_ms` calculado dinámicamente (`generated_at - captured_at` en milisegundos). Si el almacén efímero falla, responde 503 `TEMPORARILY_UNAVAILABLE`. Todas las respuestas exitosas devuelven la cabecera `Cache-Control: no-store` y `X-Request-Id`. Pruebas: 10 pruebas de integración en `crates/gs-api/tests/sessions_test.rs` (14 pruebas en total en el archivo, todas pasando). Comandos: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo test --workspace` (421 pruebas, 0 fallos; base en `main`: 411 → +10), `cargo deny check`, `cargo run -p geostream-tools -- all`: todos OK.

### 2026-10-14 — T-F4-09 — Renovación de autenticación en vivo (`AuthExpiring`, `AuthRefresh`, `AuthRefreshAck`)
- Autor/agente: Angel Nereira (con agente de código).
- Resultado: completada.
- Rama: `feat/T-F4-09-auth-refresh`.
- Requisitos: FR-AUTH-007.
- Evidencia: implementación completa del flujo de renovación de autenticación según `docs/08_STREAM_PROTOCOL.md` §6.6 y `docs/06_IDENTITY_AUTH.md` §4.3. El servidor envía el mensaje no solicitado `AuthExpiring{auth_expires_at}` con `request_id=0` al restar `auth_expiring_lead` (por defecto 60s) para la expiración de la ventana de autenticación (o de inmediato tras `ClientHello` si la ventana restante es menor o igual al lead). Ante `AuthRefresh{access_token}` de un dispositivo, se verifica criptográficamente el token y su estado en base de datos (`verify_device_token_principal`); si es válido y del mismo `device_id`, responde `AuthRefreshAck{auth_expires_at}`, actualiza scopes y época `sep` en la conexión, y posterga la expiración; si el token es inválido o ha expirado, responde `StreamError{UNAUTHENTICATED | TOKEN_EXPIRED}` manteniendo la conexión activa hasta su vencimiento anterior; si el token pertenece a otro dispositivo, responde `StreamError{PERMISSION_DENIED}` y cierra con código 4403 (`CLOSE_FORBIDDEN`). Conexiones de aplicación rechazan `AuthRefresh` con `StreamError{PERMISSION_DENIED}`. Si la ventana de autenticación vence sin renovación, la conexión se cierra con 4401 (`CLOSE_AUTH_EXPIRED`). Pruebas: 7 pruebas de integración extremo a extremo en `crates/gs-stream/tests/connection_test.rs`. Comandos: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo test --workspace` (411 pruebas, 0 fallos; base en `main`: 404 → +7), `cargo deny check`, `cargo run -p geostream-tools -- all`: todos OK.

### 2026-10-14 — T-F4-08 — Presencia, gracia de desconexión y conexión única (`Supersede`, código 4409)
- Autor/agente: Angel Nereira (con agente de código).
- Resultado: completada.
- Rama: `feat/T-F4-08-presence`.
- Requisitos: FR-PRS-002, FR-STR-007, FR-PRS-004.
- Evidencia: implementación completa del ciclo de presencia efímera, conexión única por dispositivo y gracia de desconexión según `docs/04` §6.3 y `docs/08` §6.7. En `ClientHello`, los dispositivos registran su presencia en `{prefix}:presence:{device_id}` con TTL `presence_ttl` (90s); ante conexión concurrente del mismo dispositivo, la conexión anterior se cierra con código 4409 (`CLOSE_SUPERSEDED`) localmente y se emite `DeviceBusEvent::Supersede` en `{prefix}:bus:dev:{device_id}` para instancias remotas. Conexiones de aplicación no registran presencia ni sufren supersede (FR-STR-007). Al conectar un dispositivo publicador con `sharing=true`, se difunde `MemberStatusChanged{connectivity: ONLINE}` por `{prefix}:bus:ctl:{session_id}`. La conexión renueva periódicamente su presencia (`presence_ttl / 3`) y ante `Heartbeat`. Al desconectarse, `release_presence` atómico valida `connection_id` para no anular reconexiones previas; si se libera la clave, se espera `offline_grace` (10s) en background: si no hubo reconexión, se emite `MemberStatusChanged{connectivity: OFFLINE}`; si reconectó durante la gracia, el evento `OFFLINE` se suprime limpiamente (FR-PRS-002). Miembros sin compartición activa (`sharing=false`) nunca emiten eventos de presencia (INV-PRS-1, FR-PRS-004). Pruebas: 5 pruebas de integración extremo a extremo en `crates/gs-stream/tests/connection_test.rs`. Comandos: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo test --workspace` (404 pruebas, 0 fallos; base en `main`: 399 → +5), `cargo deny check`, `cargo run -p geostream-tools -- all`: todos OK.

### 2026-10-14 — T-F4-07 — Eventos de control y revocación en vivo (`SubscriptionEnded`, cierres 4401/4403)
- Autor/agente: Angel Nereira (con agente de código).
- Resultado: completada.
- Rama: `feat/T-F4-07-control-revocation`.
- Requisitos: FR-STR-008, FR-STR-011, FR-AUTH-005.
- Evidencia: implementada la emisión de eventos de control en `gs-service` y el despacho de notificaciones en `gs-stream` (`docs/08_STREAM_PROTOCOL.md` §9, `docs/05_DOMAIN_MODEL.md` §10, `docs/06_IDENTITY_AUTH.md` §7). Las mutaciones de miembros y sesiones invocan métodos `_with_bus`: `close_session_with_bus`, `update_session_with_bus`, `expire_due_sessions_with_bus`, `add_member_with_bus`, `update_member_with_bus`, `revoke_member_with_bus` y `leave_session_with_bus` sobre `{prefix}:bus:ctl:{session_id}`; al cerrar o expirar sesión se limpian snapshots efímeros y se eleva `mver`. En revocación de credenciales (`revoke_tokens_with_bus`) y desactivación de dispositivo (`deactivate_device`) se emiten eventos en `{prefix}:bus:global`. `gs-stream::fanout` mantiene suscripción a ambos canales y despacha: ante cierre/expiración de sesión `SessionEvent` y `SubscriptionEnded{SESSION_CLOSED|SESSION_EXPIRED}` terminando la suscripción localmente; ante revocación/abandono de miembro `SubscriptionEnded{MEMBER_REVOKED|MEMBER_LEFT}` al miembro y `MemberEvent{REVOKED|LEFT}` a los restantes; ante cambio de capacidades que retira lectura (`lost_read`) `SubscriptionEnded{PERMISSION_CHANGED}` al miembro y `MemberEvent{UPDATED}` a los demás; ante `TenantDisabled`/`ApplicationDisabled` cierra conexiones activas con 4403 (`CLOSE_FORBIDDEN`); ante `ApiKeyRevoked`/`SubjectTokensRevoked`/`DeviceDeactivated` cierra con 4401 (`CLOSE_AUTH_EXPIRED`). Pruebas: 8 de integración en `crates/gs-service/tests/control_events_test.rs` y 8 de integración extremo a extremo en `crates/gs-stream/tests/connection_test.rs`. Comandos: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo test --workspace` (399 pruebas, 0 fallos; base en `main`: 383 → +16), `cargo deny check`, `cargo run -p geostream-tools -- all`: todos OK.
- Notas: Se actualizó `rustls` a 0.23.46 (parche de seguridad para RUSTSEC-2026-0349 verificado con `cargo deny check`).

### 2026-10-14 — T-F4-06 — Fan-out por bus (ruta única), coalescencia y backpressure
- Autor/agente: Angel Nereira (con agente de código).
- Resultado: completada.
- Rama: `feat/T-F4-06-fanout`.
- Requisitos: FR-STR-003, FR-STR-006.
- Evidencia: la conexión se parte en tarea lectora y tarea escritora (`04` §8); la escritora es la única dueña del sink y prioriza la cola de control acotada (`GS_STREAM_OUTBOUND_CONTROL_QUEUE`, llena → cierre 4005) sobre las ubicaciones coalescidas por (sesión, miembro), de modo que el cliente siempre recibe la más reciente (FR-STR-006, `08` §10). `gs-stream::fanout` mantiene `DashMap<SessionId, Vec<ConnHandle>>` con el subprotocolo de cada conexión para codificar cada trama con el negociado; el primer suscriptor local de una sesión suscribe a `{p}:bus:loc:{session_id}` **antes** de devolver el control (un bus de difusión pierde lo publicado antes de la suscripción) y arranca la única tarea del tema, que envuelve cada `LocationUpdate` del bus en `ServerMessage{request_id=0}` y lo deposita en `latest`; el último suscriptor libera la suscripción (refcount de `04` §6.1). Publicar recorre el mismo camino (`publish_to_bus`), así que un publicador suscrito recibe sus propias muestras. El cierre se coordina para que la escritora escriba el último frame: se le envía el `CloseFrame`, se cierra el emisor y solo entonces se cancela su token (se detectó y corrigió una carrera de `select!` entre el frame de cierre y la cancelación). Pruebas: 2 de extremo a extremo (actualización publicada que llega a un segundo suscriptor; dos publicaciones seguidas entregan la más nueva y en orden) y 6 unitarias. Comandos: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo test --workspace` (383 pruebas, 0 fallos; base en `main`: 376 → +7), `cargo run -p geostream-tools -- all`: todos OK.
- Notas: la entrega entre instancias reales llega con la implementación Redis/Valkey de T-F6-01; con el bus en memoria la ruta única ya es la misma. Pendiente de T-F4-08: presencia y `Supersede`, que hoy hacen que un publicador aparezca `OFFLINE`.

### 2026-10-14 — T-F4-05 — Publicación: precondiciones, validación, CAS de secuencia, snapshot, rate limit y acks
- Autor/agente: Angel Nereira (con agente de código).
- Resultado: completada (el fan-out multi-conexión es T-F4-06).
- Rama: `feat/T-F4-05-publishing`.
- Requisitos: FR-LOC-001, FR-LOC-002, FR-LOC-003, FR-LOC-004, FR-LOC-005, FR-LOC-006, FR-LOC-007, FR-LOC-008, FR-STR-004.
- Evidencia: `gs-service::publishing` evalúa las precondiciones de `08` §6.4 en el orden documentado (lote vacío o no creciente → `VALIDATION_FAILED`; sobredimensionado → `PAYLOAD_TOO_LARGE`; aplicación → `PERMISSION_DENIED`; bucket → `RATE_LIMITED` con `retry_after_ms`; sin membresía → `NOT_FOUND`; revocada → `MEMBER_REVOKED`; sin `publish` → `PERMISSION_DENIED`; sesión cerrada/vencida → `SESSION_*`; sin compartir → `NOT_SHARING`; otro dispositivo → `NOT_PUBLISHER`; efímero caído → `TEMPORARILY_UNAVAILABLE`) y aplica a cada muestra el algoritmo de `09` §4 con CAS atómico sobre `{p}:seq:{session}:{device}` (`current = 0` = sin expectativa, que es la semántica documentada). `received_at` lo asigna el servidor y la muestra se entrega tal cual (FR-LOC-005/006); cada aceptada refresca `{p}:snap:{session}:{member}` y eleva `{p}:devseq:{device}` con `set_if_greater`, que alimenta el `sequence_floor` del `ServerHello` siguiente (FR-LOC-008, ADR-0008). El token bucket por (conexión, sesión) vive en `gs-stream` (`publish_burst`, `min_publish_interval_ms`). Un publicador suscrito recibe sus propias `LocationUpdate` (docs/08 §7.3). Pruebas: 7 de integración contra PostgreSQL real y 5 de extremo a extremo con WebSocket real. Comandos: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo test --workspace` (376 pruebas, 0 fallos; base en `main`: 364 → +12), `cargo run -p geostream-tools -- all`: todos OK.
- Notas: `compare_and_set_sequence(key, 0, seq)` es la llamada correcta: el almacén compara atómicamente y `current = 0` significa «sin expectativa», que es exactamente la semántica de `09` §4. El snapshot se almacena con `delivery=LIVE` y se entrega como `SNAPSHOT` al leerse (`11` §3). La entrega a otros suscriptores y al bus es T-F4-06; aquí se corrige el retraso de `docs/09` §4 (el efímero se refresca con TTL de sesión).

### 2026-10-13 — T-F4-04 — Suscripción sin huecos, snapshot y `MemberStatus` con minimización
- Autor/agente: Angel Nereira (con agente de código).
- Resultado: completada.
- Rama: `feat/T-F4-04-subscription`.
- Requisitos: FR-STR-002, FR-PRS-001, FR-PRS-004.
- Evidencia: `gs-service::subscriptions` autoriza antes de leer (membresía `ACTIVE` + `read` para dispositivos, `locations:read` para aplicaciones) y devuelve `SessionInfo`, el `MemberStatus` de todos los miembros activos y el snapshot limitado a quienes comparten y tienen snapshot vigente en `{p}:snap:{session}:{member}`; el estado de sesión aplica INV-SES-1/2 (`SESSION_CLOSED` / `SESSION_EXPIRED`) y un no miembro recibe 404 sin revelar la sesión (FR-API-006). Minimización de presencia (INV-PRS-1): `sharing=false` → `connectivity=UNSPECIFIED` y sin `last_location_at`, aunque existan presencia y snapshot; `sharing=true` → `ONLINE` si existe `{p}:presence:{device}`. `gs-stream::subscription` implementa el algoritmo sin huecos de `08` §6.3 (suscripción `PENDING` → lectura → `SubscribeAck` encolado → `ACTIVE`) con cola de eventos de control acotada por `GS_STREAM_PENDING_EVENTS_MAX` y coalescencia de ubicaciones por (sesión, miembro) liberando solo las más nuevas que el snapshot; `Subscribe` repetido es idempotente, `Unsubscribe` responde siempre, el límite efectivo por conexión responde `TOO_MANY_SUBSCRIPTIONS` y un ID malformado `VALIDATION_FAILED`. Pruebas: 7 de integración contra PostgreSQL real (`crates/gs-service/tests/subscriptions_test.rs`) y 6 de extremo a extremo con WebSocket real (`crates/gs-stream/tests/connection_test.rs`: ack con sesión y miembros, no miembro → `NOT_FOUND`, miembro sin `read` → `PERMISSION_DENIED`, aplicación sin `locations:read` → `PERMISSION_DENIED`, sesión cerrada → `SESSION_CLOSED`, límite de suscripciones → `TOO_MANY_SUBSCRIPTIONS`), más 6 unitarias de la cola y la coalescencia. Comandos: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo test --workspace` (364 pruebas, 0 fallos; base en `main`: 345 → +19), `cargo run -p geostream-tools -- all`: todos OK.
- Notas: el fan-out que entrega `LocationUpdate` y los eventos de control a las suscripciones activas es T-F4-06; la cola pendiente ya está en su sitio para que ese cableado no pierda eventos. La presencia todavía no la escribe nadie (T-F4-08), por lo que un publicador aparece `OFFLINE` hasta entonces.

### 2026-10-13 — T-F4-03 — `AuthzCache` con `mver`, invalidación por bus y revalidación del principal
- Autor/agente: Angel Nereira (con agente de código).
- Resultado: completada.
- Rama: `feat/T-F4-03-authz-cache`.
- Requisitos: NFR-SEC-002 (plazo de revocación).
- Evidencia: `gs-service::authz` implementa la caché por instancia de `04` §6.2 con entradas `(session_id, subject_id)` para dispositivos y `(session_id, application_id)` para aplicaciones, TTL `GS_AUTHZ_CACHE_TTL_MS`, revalidación por `{p}:mver:{session}` (renueva si no cambió, recarga si cambió), `invalidate_session` como acelerador del bus, `note_global_bus_event` para los eventos de `{p}:bus:global`, `flush` al reconectar y `revalidate_principal` con el mismo TTL. La conexión de `gs-stream` revalida su principal al procesar mensajes o por temporizador: token revocado o dispositivo desactivado → cierre 4401; tenant o aplicación deshabilitados → 4403. Para detectar la revocación de tokens sin conservar el token, el `TicketGrant` añade el campo opcional `token_epoch` (documentado en `06` §5 y `11` §3 en el mismo PR, `00` §1.2). Métrica `gs_authz_cache_events_total{event}` con `hit`/`miss`/`revalidate`/`reload`/`flush`. Pruebas: 7 de integración contra PostgreSQL real (`crates/gs-service/tests/authz_test.rs`) y 2 de extremo a extremo con WebSocket real (`crates/gs-stream/tests/connection_test.rs`), más 4 unitarias de invalidación. Comandos: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo test --workspace` (345 pruebas, 0 fallos; base en `main`: 332 → +13), `cargo run -p geostream-tools -- all`: todos OK.
- Notas: `gs-ephemeral::get_counter` (añadida en T-F4-02) es la lectura que usa `sequence_floor`; aquí se reutiliza `get_membership_version` para `mver`. El cableado de `{p}:bus:ctl:{session_id}` a `invalidate_session` llega con las suscripciones de T-F4-04.

### 2026-10-13 — T-F4-02 — Conexión de stream: upgrade, subprotocolos, hello, máquina de estados, heartbeat y cierres
- Autor/agente: Angel Nereira (con agente de código).
- Resultado: completada (el montaje del router en `geostream serve` queda bloqueado por DR-20261013-01).
- Rama: `feat/T-F4-02-stream-connection`.
- Requisitos: FR-STR-001, FR-STR-005.
- Evidencia: `gs-api::stream` publica `GET /v1/stream` con las comprobaciones previas al upgrade de `08` §3 en orden subprotocolo (400) → rate limit de upgrades por IP (429 con `Retry-After`) → límite de conexiones por instancia (503 con `Retry-After`) → canje atómico del ticket (401) → estado de tenant y aplicación (403), todas como `application/problem+json` y sin consumir ticket cuando se rechaza. La negociación de subprotocolo vive en `gs-domain` (regla pura compartida) y el codec en `gs-stream` (Prost binario, JSON canónico proto3 en texto). `gs-stream::connection` implementa la máquina de estados `AWAITING_HELLO → READY → CLOSED`: `ClientHello` validado (minor negociado, cadenas imprimibles acotadas) y `ServerHello` con `connection_id`, `server_time`, `auth_expires_at`, `sequence_floor` leído de `{p}:devseq:{device}` (0 para aplicaciones), límites efectivos de `17` §2.6 combinados con la política del tenant e IDs del principal; heartbeat con `HeartbeatAck`; cierres 4002 (sin hello), 4001 (minor no soportado), 4000 (trama de tipo incorrecto, mensaje no válido antes del hello o presupuesto de errores de protocolo agotado), 1009 (trama grande), 4429 (abuso sostenido), 4408 (inactividad), 4401 (ventana de autenticación vencida) y 4500 (interno), con el nombre del código como razón. Métricas de `16` §4.2 emitidas. `gs-ephemeral` añade `get_counter` con caso en la suite común (la implementación Redis/Valkey de T-F6-01 debe ejecutarla). Pruebas: 16 de integración extremo a extremo en `crates/gs-stream/tests/connection_test.rs` contra un listener real con `tokio-tungstenite`, más unitarias de negociación, codec, configuración, cierres y contador de conexiones. Comandos: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo test --workspace` (332 pruebas, 0 fallos; base en `main`: 299 → +33), `cargo run -p geostream-tools -- all`: todos OK.
- Notas: se abre **DR-20261013-01** porque montar el router en `geostream serve` exige que el binario cargue `ServerConfig` y `04` §4.1 (codificado en `tools/check_crate_deps`) no le permite depender de `gs-config`. El comportamiento queda completo y probado a nivel de librería, que es exactamente lo que `serve` montará. `gs-api` se usa como dev-dependency de `gs-stream` solo en la suite de integración (el checker gobierna dependencias normales, igual que `gs-service` usa `sqlx` para probar).

### 2026-10-13 — T-F4-01 — Tickets de stream: `POST /v1/stream/tickets` y canje atómico
- Autor/agente: Angel Nereira (con agente de código).
- Resultado: completada (el cableado del handler de upgrade al WebSocket es T-F4-02).
- Rama: `feat/T-F4-01-stream-tickets`.
- Requisitos: FR-AUTH-006.
- Evidencia: `gs-auth::ticket` genera y valida el formato `^gst_[A-Za-z0-9_-]{43}$` (32 bytes de CSPRNG en base64url sin padding) y expone el digest SHA-256; `gs-service::tickets` emite el ticket guardándolo **solo** como digest en `{p}:ticket:{sha256_hex}` con TTL `GS_TICKET_TTL_S`, construye el `TicketGrant` (`principal_kind`, `tenant_id`, `application_id`, `subject_id`/`device_id` o `api_key_id`, scopes, `auth_expires_at` = `exp` del token de dispositivo o `now + GS_STREAM_APP_MAX_CONNECTION_AGE_S` para aplicación) y lo canjea con get-and-delete atómico; `verify_ticket_principal_state` revalida tenant y aplicación `ACTIVE` (403 `ACCOUNT_DISABLED`, `08` §3). `gs-api::stream_tickets` publica `POST /v1/stream/tickets` → 201 `StreamTicketResponse` (`ticket`, `expires_at`, `stream_url` desde `GS_PUBLIC_BASE_URL`) con `Cache-Control: no-store`, `X-Request-Id` y exigencia del scope `stream:connect` para ambos principales. Pruebas: 5 unitarias en `gs-auth`, 9 de integración contra PostgreSQL real en `crates/gs-service/tests/tickets_test.rs` (uso único, expiración por TTL con espera real, ventana de autenticación vencida, ticket desconocido o malformado, tenant/aplicación deshabilitados, clave por digest, roundtrip del grant) y 4 HTTP en `crates/gs-api/tests/stream_ticket_test.rs` con validación de todas las respuestas contra el contrato OpenAPI. Comandos: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo test --workspace` (299 pruebas, 0 fallos), `cargo run -p geostream-tools -- all`: todos OK. Refactor sin cambio de comportamiento: el cargador del contrato OpenAPI de `conformance_test.rs` se extrajo a `tests/common/spec.rs` para reutilizarlo desde la suite nueva (`docs/18` §5).
- Notas: `redeem_stream_ticket` + `verify_ticket_principal_state` son la pieza que consume el upgrade; su cableado en el handler WebSocket (negociación de subprotocolo, límites, códigos de cierre) pertenece a T-F4-02. Sigue pendiente el montaje del router REST completo en `geostream serve` (hoy solo se sirven `/health/*`), necesario para los escenarios de F4.

### 2026-10-13 — T-F3-11 — Conformidad OpenAPI, matriz de autorización y aislamiento de tenants
- Autor/agente: Angel Nereira (con agente de código).
- Resultado: completada.
- PR: main (commit incremental).
- Requisitos: FR-API-001, FR-API-005, FR-API-006; NFR-SEC-004 en curso (mensajes de stream pendientes en F4).
- Evidencia: `conformance_test.rs` (2 pruebas) valida request/response contra OpenAPI y ejecuta las matrices; destapó y corrigió dos defectos reales (`listInvitations`/`revokeInvitation` sin 404). Comandos: fmt, clippy -D warnings, `cargo test --workspace`, `cargo deny check`, `geostream-tools all`: todos OK. El test genera `target/openapi/geostream.v1.json` con `npx @redocly/cli bundle` si falta o está desactualizado (requiere Node.js). Se añadió `jsonschema` (lista aprobada) como dev-dependency de `gs-api` y un `allow(clippy::too_many_arguments)` acotado en un helper de test.

### 2026-10-13 — T-F3-10 — Rate limits REST e invitaciones
- Autor/agente: Angel Nereira (con agente de código).
- Resultado: completada.
- PR: main (commit incremental).
- Requisitos: FR-API-004.
- Evidencia: módulo `gs-api::rate_limit` con los 4 ámbitos de `docs/07` §8 (`rest_device`, `rest_app`, `invite_redeem`, `auth_ip`), ventana fija 60 s, 429 `RATE_LIMITED` con `Retry-After` y cabeceras `RateLimit-*`, métrica `gs_rate_limited_total`. Comandos: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo test --workspace`, `cargo deny check`, `cargo run -p geostream-tools -- all`: todos OK. Se amplió el límite de lote de `expire_due_sessions` en `audit_test` (10 → 10000) por ser frágil con filas vencidas residuales.

### 2026-10-13 — T-F3-09 — Auditoría de acciones sensibles
- Autor/agente: Angel Nereira (con agente de código).
- Resultado: completada.
- PR: main (commit incremental).
- Requisitos: FR-PRV-005, FR-SHR-005.
- Evidencia: Implementación integral del subsistema de auditoría de acciones sensibles en `gs-service::audit` y `gs-store::AuditRepository` para el ciclo de vida de sesiones, membresías, invitaciones, subjects y dispositivos según `docs/10` §8.2, §8.3 y `docs/11` §2. Registro normativo de acciones: `session.created|updated|closed|expired`, `member.added|updated|revoked|left`, `invitation.created|revoked|redeemed`, `sharing.started|stopped`, `subject.tokens_revoked` y `device.deactivated`. Cumplimiento estricto de `FR-PRV-005` (auditoría de acciones sensibles con actor_kind, actor_id, subject_id, target_kind, target_id, outcome, request_id y details estructurados) y `FR-SHR-005` (auditoría de inicio y cese de compartición con actor, membresía, publisher device id, valor booleano e instante, con total ausencia de campos de posición o coordenadas). Aplicación rigurosa de las reglas de privacidad de `docs/10` §8.2: total exclusión de campos de coordenadas/posición, tokens, tickets, API keys, secretos de autenticación, códigos de invitación en claro, metadata de sesión y `external_subject_id` sin anonimizar en los detalles JSON de auditoría. Garantía estricta de aislamiento por tenant (`INV-TEN-1`) con métodos de repositorio `list_by_action`, `list_by_target` y `count_by_tenant` filtrados por `tenant_id`. Suite exhaustiva de pruebas de integración en `crates/gs-service/tests/audit_test.rs` con 2 tests: `test_fr_prv_005_and_fr_shr_005_sensitive_actions_audited` (cubriendo exhaustivamente todas las acciones sensibles y verificando de forma recursiva con `assert_no_forbidden_fields` que ninguna clave prohibida exista en `details`) y `test_inv_ten_1_audit_strict_tenant_isolation` (verificando la estricta ausencia de filtraciones entre tenants en consultas de auditoría). Gates de calidad 100% verdes: `cargo fmt`, `cargo clippy` (`-D warnings`), `cargo test --workspace` (223 pruebas verdes), `cargo deny check`, `geostream-tools all`.

### 2026-10-13 — T-F3-08 — Deshabilitar/habilitar tenant y aplicación por CLI con invalidación de cachés
- Autor/agente: Angel Nereira (con agente de código).
- Resultado: completada.
- PR: main (commit incremental).
- Requisitos: FR-ADM-003.
- Evidencia: Implementación completa del ciclo de deshabilitar y habilitar tenants y aplicaciones tanto por CLI (`geostream admin tenant disable|enable`, `geostream admin app disable|enable`) como a través de la capa de servicio con invalidación en bus global (`set_tenant_enabled_with_bus`, `set_application_enabled_with_bus`, `revoke_api_key_with_bus`) y bloqueo estricto en el plano de control REST (`FR-ADM-003`, `docs/06` §7, `docs/16` §3). Publicación de eventos de invalidación en `{prefix}:bus:global` (`TenantDisabled`, `TenantEnabled`, `ApplicationDisabled`, `ApplicationEnabled`, `ApiKeyRevoked`, y `SubjectTokensRevoked` en revocación de tokens) con formato JSON normativo y timestamp RFC 3339. Diferenciación estricta en el modelo de autenticación (`gs-service::auth::AuthError`): ante un tenant o aplicación desactivada (`status != Active`), las peticiones REST con credencial de aplicación (API key) o token de dispositivo devuelven de inmediato HTTP 403 `ACCOUNT_DISABLED` con Problem Details RFC 9457 (`status=403`, `code=ACCOUNT_DISABLED`), mientras que fallos de época de subject, dispositivo desactivado o API key revocada/expirada devuelven HTTP 401 `UNAUTHENTICATED`. La reactivación de un tenant o aplicación restablece el acceso REST de forma inmediata sin latencia ni retención obsoleta. Suites de pruebas de integración completas: prueba de ciclo de vida de aplicación con `enable` en `apps/geostream/tests/admin_test.rs`, pruebas de publicación y recepción de eventos en bus en `crates/gs-service/tests/admin_test.rs`, pruebas de rechazo paso 4a/4b/4c en `crates/gs-service/tests/tokens_test.rs`, y suite REST de extremo a extremo en `crates/gs-api/tests/account_disabled_test.rs` verificando respuestas 403/200 tanto para API keys como para tokens JWT de dispositivo. Gates de calidad: `cargo fmt`, `cargo clippy`, `cargo test --workspace` (221 pruebas verdes), `cargo deny check` y `geostream-tools all` exitosos.

### 2026-10-13 — T-F3-07 — Subjects y dispositivos: búsqueda, consulta, revocación de tokens, desactivación de dispositivo
- Autor/agente: Angel Nereira (con agente de código).
- Resultado: completada.
- PR: main (commit incremental).
- Requisitos: FR-AUTH-005.
- Evidencia: Implementación completa del ciclo de vida y gestión de subjects y dispositivos en plano de control REST (`GET /v1/subjects?external_subject_id=...`, `GET /v1/subjects/{subject_id}`, `POST /v1/subjects/{subject_id}/revoke-tokens`, `GET /v1/devices/{device_id}`, `DELETE /v1/devices/{device_id}`) en `gs-store` (`SubjectRepository::revoke_tokens`, `DeviceRepository::deactivate_and_stop_publishing`), `gs-service` (`subjects`) y `gs-api` (`subjects`). Búsqueda estricta por `external_subject_id` con validación de caracteres (formato Crockford / alfanumérico) y rechazo 422 `VALIDATION_FAILED` ante query params desconocidos o faltantes. Autorización estricta por rol y matriz de permisos (`docs/06` §8.2): aplicaciones requieren scope `subjects:manage` (403 `INSUFFICIENT_SCOPE`), dispositivos denegados para operaciones de subject (403 `PERMISSION_DENIED`). Dispositivos restringidos a sus propios recursos: `getDevice` restringido a dispositivos del mismo subject (404 `NOT_FOUND` para otros sujetos por no enumeración `FR-API-006`), `deactivateDevice` restringido a `own_device` (404 `NOT_FOUND` para otros). Revocación atómica de tokens de subject incrementando `token_epoch` en base de datos; las peticiones posteriores con tokens emitidos con epoch anterior son rechazadas con 401 `UNAUTHENTICATED` (`AccountDisabled`). Desactivación de dispositivo idempotente (204 No Content), cesando automáticamente la compartición en todas las sesiones donde actuaba como publicador activo, limpiando snapshots de ubicación en almacén efímero con hasta 3 reintentos, incrementando la versión de membresías (`mver++`) y emitiendo el evento de control `SharingStopped` en el bus. Suites de integración completas: 4 pruebas en `gs-service` y 3 pruebas en `gs-api` contra PostgreSQL real. Gates de calidad: `cargo fmt`, `cargo clippy`, `cargo test --workspace` (218 pruebas verdes), `cargo deny check` y `geostream-tools all` exitosos.

### 2026-10-13 — T-F3-06 — Invitaciones: crear, listar, revocar, canjear con concurrencia y rate limit
- Autor/agente: Angel Nereira (con agente de código).
- Resultado: completada.
- PR: main (commit incremental).
- Requisitos: FR-INV-001, FR-INV-002, FR-INV-003.
- Evidencia: Implementación completa del plano de control REST y ciclo de vida de invitaciones (`POST /v1/sessions/{session_id}/invitations`, `GET /v1/sessions/{session_id}/invitations`, `DELETE /v1/sessions/{session_id}/invitations/{invitation_id}`, `POST /v1/invitations/redeem`) en `gs-domain` (`InvitationStatus`, `status_at`), `gs-store` (`InvitationRepository`), `gs-service` (`invitations`) y `gs-api` (`invitations`). Generación uniforme de códigos en Crockford Base32 sin caracteres ambiguos con almacenamiento exclusivo del digest SHA-256 (`docs/05` §6). Validación estricta de parámetros: `max_uses` (1..=`invitation_max_uses`) y TTL (`60..=invitation_max_ttl_seconds`, acotado por la expiración de la sesión). Reglas de capacidades del emisor: las capacidades otorgadas deben ser un subconjunto estricto de las que posee el emisor y la concesión de `manage` solo está autorizada para el propietario (`role=OWNER`) o una aplicación. Paginación por cursor opaco (`created_at DESC, id DESC`) y filtrado por emisor (`manage_or_invite_own`). Revocación idempotente autorizada para gestores o el emisor. Canje atómico en PostgreSQL con bloqueo de filas (`SELECT ... FOR UPDATE`), evaluando la precedencia de estados (`REVOKED > EXPIRED > EXHAUSTED > ACTIVE`, `docs/07` §9.4), detección de membresía previa activa (409 `CONFLICT`), validación de cupo de la sesión (409 `SESSION_FULL`), rate limit por dispositivo en ventana de 60 s contra `GS_INVITE_REDEEM_RPM` con cabecera `Retry-After` (429 `RATE_LIMITED`), incremento atómico de la versión de membresías (`mver++`) y persistencia de respuestas idempotentes cifradas en reposo con AES-256-GCM. Suites de integración completas: 3 pruebas en `gs-service` y 2 pruebas en `gs-api` contra PostgreSQL real. Gates de calidad: `cargo fmt`, `cargo clippy`, `cargo test --workspace` (214 pruebas verdes), `cargo deny check` y `geostream-tools all` exitosos.

### 2026-10-13 — T-F3-05 — Compartición: `PUT …/sharing` con efectos (borrado de snapshot, `mver++`, evento de control)
- Autor/agente: Angel Nereira (con agente de código).
- Resultado: completada.
- PR: main (commit incremental).
- Requisitos: FR-SHR-001, FR-SHR-002, FR-SHR-003, FR-SHR-004.
- Evidencia: Implementación completa del plano de control de compartición de ubicación (`PUT /v1/sessions/{session_id}/members/{member_id|me}/sharing`) en `gs-store` (`MembershipRepository::set_sharing`), `gs-service` (`members::set_sharing`) y `gs-api` (`members::set_sharing_handler`). Restricción estricta de inicio de compartición exclusivamente al propio subject con dispositivo activo, capacidad `publish` y scope de token `sharing:write` (`INV-SHR-2`, `FR-SHR-002`), garantizando un único dispositivo publicador por membresía con transferencia atómica de rol si se invoca desde otro dispositivo del mismo subject (`FR-SHR-004`), e idempotencia sin eventos si se repite desde el mismo dispositivo (`INV-SHR-3`). Detención de compartición (`sharing=false`) autorizada para el propio subject, miembros con `can_manage=true` o aplicaciones con scope `members:write` (`FR-SHR-003`). Efectos síncronos antes de responder: borrado inmediato del snapshot de última ubicación en almacén efímero con reintentos automáticos (hasta 3 intentos, `NFR-PRIV-002`, `docs/11` §2.1), incremento atómico de la versión de membresías (`mver++`) y publicación del evento de control en el bus (`SharingStarted`, `SharingTransferred`, `SharingStopped`) en `{prefix}:bus:ctl:{session_id}`. Suites de integración: 6 pruebas unitarias/de servicio en `gs-service` y 6 pruebas de integración REST en `gs-api` contra PostgreSQL real. Gates de calidad: `cargo fmt`, `cargo clippy`, `cargo test --workspace` (211 pruebas verdes), `cargo deny check` y `geostream-tools all` exitosos.

### 2026-10-13 — T-F3-04 — Membresías: alta, listado, consulta (`me`), cambio de capacidades/rol, revocación, salida, transferencia de propiedad, readmisión
- Autor/agente: Angel Nereira (con agente de código).
- Resultado: completada.
- PR: main (commit incremental).
- Requisitos: FR-MEM-001, FR-MEM-002, FR-MEM-003, FR-MEM-004, FR-MEM-005, FR-MEM-006, FR-MEM-007, FR-SES-007.
- Evidencia: Implementación completa del ciclo de vida de membresías en plano de control REST (`POST /v1/sessions/{session_id}/members`, `GET /v1/sessions/{session_id}/members`, `GET /v1/sessions/{session_id}/members/{member_id}`, `GET /v1/sessions/{session_id}/members/me`, `PATCH /v1/sessions/{session_id}/members/{member_id}`, `DELETE /v1/sessions/{session_id}/members/{member_id}`, `POST /v1/sessions/{session_id}/leave`) en `gs-store` (`MembershipRepository`), `gs-service` (`members`) y `gs-api` (`members`). Incluye verificación atómica de límite de miembros con bloqueo `FOR UPDATE` sobre la fila de sesión (INV-MEM-7), soporte de readmisión de miembros `LEFT` y `REVOKED` (FR-MEM-007), control de concurrencia optimista con precondición `If-Match`, inmutabilidad de capacidades del propietario (INV-MEM-1), prohibición de auto-modificación (INV-MEM-3), regla anti-escalada de capacidades (INV-MEM-2), cese inmediato de compartición al retirar `publish` (INV-MEM-4), transferencia atómica de rol `OWNER` (FR-MEM-005), revocación idempotente (204 No Content), abandono de sesión por dispositivo y cascada de salida del propietario al miembro más antiguo con `can_manage=true` o cierre/limpieza de sesión si no hay sustituto (FR-SES-007). Suites de integración: 5 pruebas en `gs-service` y 5 pruebas en `gs-api` contra PostgreSQL real. Gates de calidad: `cargo fmt`, `cargo clippy`, `cargo test --workspace` (210 pruebas verdes), `cargo deny check` y `geostream-tools all` exitosos.

### 2026-10-12 — T-F3-03 — Sesiones: control plane REST, control de concurrencia y expiración
- Autor/agente: Angel Nereira (con agente de código).
- Resultado: completada.
- PR: main (commit incremental).
- Requisitos: FR-SES-001, FR-SES-002, FR-SES-003, FR-SES-004, FR-SES-005, FR-SES-006.
- Evidencia: Implementación completa del plano de control REST de sesiones (`POST /v1/sessions`, `GET /v1/sessions`, `GET /v1/sessions/{session_id}`, `PATCH /v1/sessions/{session_id}`, `POST /v1/sessions/{session_id}/close`) en `gs-store` (`SessionRepository`, `MembershipRepository`), `gs-service` (`sessions`) y `gs-api` (`sessions`). Soporta creación por aplicación (con o sin `owner_external_subject_id` e `initial_members`) y por dispositivo (propietario automático con las 4 capacidades, rechazo 422 de parámetros no permitidos), control de concurrencia optimista mediante `If-Match` y cabeceras `ETag`, listado paginado con cursor opaco y filtros (`status`, `external_subject_id` con rechazo 422 si los envía un dispositivo), lectura restringida a miembros activos para dispositivos (404 si no pertenece), cierre idempotente (INV-SES-3), verificación perezosa de expiración en lectura (INV-SES-2) y job de expiración en segundo plano (`expire_due_sessions`, `expiry_loop`). Pruebas de integración: 5 en `gs-service` y 5 en `gs-api` contra PostgreSQL real. Gates de calidad: `cargo fmt`, `cargo clippy`, `cargo test --workspace` (200 pruebas verdes), `cargo deny check` y `geostream-tools all` exitosos.

### 2026-10-12 — T-F2-07 — `gs-api` esqueleto: router, `X-Request-Id`, Problem Details, principal, límites, salud y métricas
- Autor/agente: Angel Nereira (con agente de código).
- Resultado: completada.
- PR: main (commit incremental).
- Requisitos: FR-OPS-002, FR-API-001 (base).
- Evidencia: `gs-api` implementa (1) `problem`: Problem Details con `type`, `title` fija por código, `status`, `code`, `detail` acotado a 200 caracteres, `request_id` y `errors` solo en `VALIDATION_FAILED`, renderizado como `application/problem+json`; (2) `security`: extracción del principal desde `Authorization: Bearer` clasificando `gsk_…` (aplicación) y JWT (dispositivo), con el credencial en un `Zeroizing` cuyo `Debug` es `<redacted>` y mapeo de errores a `UNAUTHENTICATED`; (3) `router`: middleware que propaga o genera `X-Request-Id: req_…`, `DefaultBodyLimit`, log de acceso que solo registra método, ruta, presencia de credencial, estado y duración (nunca `Authorization` ni cuerpos), `/health/live`, `/health/ready` con sonda de dependencias (PostgreSQL con timeout de 1 s → 503) y `/metrics` Prometheus montado con `PrometheusRenderer` sobre el listener de administración. `geostream serve` usa el router nuevo con la sonda real de base de datos. Pruebas: 11 unitarias y 7 de integración contra un listener real (id de correlación generado y propagado, ready degradado → 503, cuerpo oversized → 413, Problem Details de credencial ausente); `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo test --workspace --all-features` (184 pruebas, 0 fallos), `cargo deny check`, `cargo audit` y `geostream-tools all` verdes.
- Notas: `gs-domain` se usa a través de `pub use gs_domain;` en `gs-service` porque `tools/check_crate_deps` solo permite `{gs-service}` a `gs-api`; es la misma fachada que ya existía para `migrate` y `admin`. El formato JSON de los logs de acceso sigue bloqueado por DR-20261012-04.

### 2026-10-12 — T-F2-06 — `gs-observability`: tipos redactados, métricas base, logs JSON y verificador de captura
- Autor/agente: Angel Nereira (con agente de código).
- Resultado: completada salvo el cableado en `serve` (bloqueado por DR-20261012-04).
- PR: main (commit incremental).
- Requisitos: FR-PRV-004 (base), NFR-OBS-001, NFR-OBS-002, NFR-SEC-003.
- Evidencia: `gs-observability` implementa (1) los newtypes de secreto `ApiKeySecret`, `AccessToken`, `Ticket`, `InvitationCode` y `WebhookSecret` con `Debug`/`Display` redactados y `Zeroize` al soltarse (`docs/10` §8.2); (2) el catálogo completo de métricas y etiquetas de `docs/16` §4.2 (40 nombres de métrica, 22 nombres de etiqueta permitidos y enums acotados `PrincipalKind`, `StatusClass`, `RateLimitScope`, `SampleResult`, `TopicKind`, `AuthzCacheEvent`), con rechazo explícito de cualquier etiqueta con identificador de sesión, miembro, dispositivo, subject o tenant (NFR-OBS-002); (3) el formateador de logs JSON con exactamente los campos de `docs/16` §4.3 más `init_from_env`, que respeta `GS_LOG_FORMAT`, `GS_LOG_LEVEL` y `GS_ENV` y falla si `GS_LOG_UNSAFE_LOCATIONS=true` fuera de `dev`; y (4) el verificador de captura de `docs/18` §7, que busca las coordenadas sintéticas de los vectores y los patrones `gsk_`, `gst_`, `whsec_`, `eyJ`, con informe de línea y fragmento acotado. Suite: 15 pruebas unitarias y 3 de integración, incluida una que instala el formateador real sobre un `tracing` subscriber, comprueba los 8 campos documentados y verifica que el newtype de secreto solo aparece como `<redacted>`; `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo test --workspace --all-features` (166 pruebas, 0 fallos), `cargo deny check`, `cargo audit` y `geostream-tools all` verdes.
- Notas: `geostream serve` mantiene el subscriber a secas porque `tools/check_crate_deps` no permite que el binario dependa de `gs-observability`; DR-20261012-04 abierta para que el propietario decida entre enmendar el checker (recomendado, alinea con la frase normativa de `04` §4.1) o enmendar el documento. No se desactivó ningún check.

### 2026-10-12 — T-F2-05 — `gs-ephemeral` y `gs-bus`: traits e implementación en memoria con suite parametrizada
- Autor/agente: Angel Nereira (con agente de código).
- Resultado: completada.
- PR: main (commit incremental).
- Requisitos: — (base para FR-STR-009, FR-PRS-003, NFR-RES-002).
- Evidencia: `gs-ephemeral` define el rasgo `EphemeralStore` con las operaciones del keyspace de `docs/11` §3 (snapshot con TTL y borrado inmediato, CAS de secuencia según `docs/09` §4, `set_if_greater`, presencia con liberación condicional por `connection_id`, versión de membresías, tickets de un uso y contadores de rate limit) y la implementación en memoria `InMemoryEphemeralStore` con expiración diferida y estado `is_ready` para la degradación de `docs/04` §7.6; `gs-bus` define el rasgo `Bus` (suscripción, baja, publicación, recepción, conteo de suscripciones por tema y salud de conexión) y la implementación en memoria `InMemoryBus` sobre canales `broadcast` con refcount por tema. La suite de pruebas vive en `tests/common/` de cada crate y se ejecuta contra la implementación en memoria, de modo que la implementación Redis/Valkey de T-F6-01 reutilice exactamente los mismos casos (ADR-0010). Pruebas verdes: 2 en `gs-ephemeral` y 3 en `gs-bus`; `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo test --workspace --all-features` (148 pruebas, 0 fallos), `cargo deny check`, `cargo audit` y `geostream-tools all` verdes.
- Notas: ninguna dependencia nueva fuera de la lista aprobada (`dashmap`, `time` y `tokio` ya figuraban). La semántica de `GS_STATE_BACKEND=memory` (una sola instancia) queda documentada en el código y probada (dos instancias no comparten estado).

### 2026-10-12 — T-F2-04 — `gs-auth`: API keys (formato, hash, tiempo constante), JWT EdDSA, rotación y scopes
- Autor/agente: Angel Nereira (con agente de código).
- Resultado: completada.
- PR: main (commit incremental).
- Requisitos: FR-AUTH-001, FR-AUTH-003, FR-AUTH-004, FR-AUTH-008.
- Evidencia: `gs-auth` implementa el formato y verificación del credencial de API key (`gsk_<key_id>.<secret>` con parseo estricto sin consultar la base de datos y comparación en tiempo constante con `subtle`), la emisión y verificación de JWT EdDSA con los 12 claims exactos de `contracts/test-vectors/token_claims.json` (sin claims adicionales, exigidos con `deny_unknown_fields`), el orden de verificación de `docs/06` §4.3 pasos 1–3 (forma compacta y `kid`, firma EdDSA, `iss`/`aud`/`nbf`/`exp` con `GS_JWT_LEEWAY_S`), el mapeo de errores a `ErrorCode` (`TOKEN_EXPIRED` vs `UNAUTHENTICATED`), y el conjunto de claves de firma (`SigningKeySet`) que carga `GS_JWT_KEYS_DIR`, verifica con todos los pares públicos durante la ventana de rotación y exige privada válida para `GS_JWT_ACTIVE_KID`. Suite `crates/gs-auth/tests/`: 7 pruebas de API key, 7 de scopes, 5 de firma y 9 de JWT (incluido el vector `token_claims.json` y los negativos de token expirado, futuro, `kid` desconocido, emisor/audiencia incorrectos, claims extra y rotación con par antiguo vigente); `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo test --workspace --all-features` (143 pruebas, 0 fallos), `cargo deny check`, `cargo audit` y `geostream-tools all` verdes.
- Notas: los pasos 4 y 5 de `docs/06` §4.3 (estado de tenant/aplicación/subject/dispositivo y comprobación de scope) requieren la verdad en PostgreSQL y se implementan en la frontera de autenticación de `gs-service` (T-F2-07/T-F2-08); `gs-auth` cubre la parte criptográfica y de forma.

### 2026-10-12 — T-F2-03 — CLI `geostream admin` (tenant, app, key, policy, signing-key)
- Autor/agente: Angel Nereira (con agente de código).
- Resultado: completada.
- PR: main (commit incremental).
- Requisitos: FR-ADM-001, FR-ADM-002.
- Evidencia: implementado el CLI completo de `docs/16_OPERATIONS.md` §3 en `apps/geostream/src/admin.rs` y los casos de uso en `crates/gs-service/src/admin.rs`: `tenant create|list|disable|enable`, `app create|list|disable|enable`, `key create|list|revoke`, `policy get|set` y `signing-key generate`; `session close` queda diferido a T-F3-03 con mensaje explícito. `key create` genera el secreto de 256 bits (`gs_auth::api_key`), almacena solo su SHA-256 e imprime el credencial `gsk_…` una sola vez; `key list` nunca muestra secretos. `policy set` valida contra `gs-config` (`gs_auth` reexporta `TenantPolicy`), rechaza claves desconocidas, tipos incorrectos y valores fuera de rango con mensaje explícito, y guarda únicamente los overrides para que las claves ausentes tomen el default (FR-ADM-002). Todas las mutaciones quedan auditadas con `actor_kind=OPERATOR` y las acciones de `docs/10` §8.3. `gs-auth` añade catálogos de scopes (aplicación `06` §3.4 y dispositivo `06` §6), generación y hash de secretos y generación de pares Ed25519 PKCS#8/SPKI en PEM con fichero privado en modo `0600`. Pruebas: `crates/gs-auth/tests/{api_key,scopes,signing_key}_test.rs` (16), `crates/gs-service/tests/admin_test.rs` (9) y `apps/geostream/tests/admin_test.rs` (8) verdes contra PostgreSQL 18 real; `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo test --workspace --all-features` (131 pruebas, 0 fallos), `cargo deny check`, `cargo audit` y `geostream-tools all` verdes.
- Notas: `upsert` de entidades por el operador (`app disable/key …` sin `--tenant`) sigue las firmas de `16` §3 derivando el tenant de la entidad; las escrituras filtran por el tenant resuelto. DR-20261012-03 abierta para confirmar esa lectura. La publicación en el bus de `TenantDisabled`/`ApplicationDisabled`/`ApiKeyRevoked` queda diferida a T-F2-05 (un proceso CLI con `GS_STATE_BACKEND=memory` no tiene a quién notificar).

### 2026-10-12 — T-F2-02 — Repositorios de tenants, aplicaciones, API keys, subjects, dispositivos y auditoría en `gs-store`
- Autor/agente: Angel Nereira (con agente de código).
- Resultado: completada.
- PR: main (commit incremental).
- Requisitos: FR-AUTH-001 (persistencia).
- Evidencia: implementados en `crates/gs-store` los modelos de entidad (`Tenant`, `Application`, `ApiKey`, `Subject`, `Device`, `AuditEvent` con sus enums de estado y `Debug` redactado del hash secreto) y seis repositorios PostgreSQL (`TenantRepository`, `ApplicationRepository`, `ApiKeyRepository`, `SubjectRepository`, `DeviceRepository`, `AuditRepository`) con filtro obligatorio por `tenant_id` en toda consulta y mutación (INV-TEN-1); upsert atómico de subject por `(tenant_id, external_subject_id)` activo (INV-SUB-1), incremento de `token_epoch` para revocación global, borrado lógico `ERASED` con anonimización del identificador externo; upsert de dispositivo por `(application_id, installation_id, subject_id)` (INV-DEV-1) y aplicación de `max_devices_per_subject` desactivando el más antiguo por `last_seen_at` (INV-DEV-2); ciclo de vida de API keys (crear, consultar, listar por aplicación, revocar terminal, `last_used_at`) y repositorio de auditoría con consulta por tenant y por subject. Suite `crates/gs-store/tests/repos_test.rs` con 6 pruebas de integración contra PostgreSQL 18 real (nombres con el ID de invariante y `// covers:`); `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo test --workspace --all-features` (100 pruebas, 0 fallos), `cargo deny check` y `cargo audit` (559 dependencias, sin vulnerabilidades) y `geostream-tools all` verdes.
- Notas: `upsert_device` usa `UpsertDeviceParams` para respetar `clippy::too_many_arguments` (decisión interna de `19` §7, sin cambio de comportamiento observable). Abierta DR-20261012-02 sobre la ubicación normativa de las pruebas de las cuatro invariantes de persistencia (`05` §11).

### 2026-10-12 — T-F2-01 — Migración inicial 0001_init.sql, subcomando geostream migrate y verificación de esquema
- Autor/agente: Angel Nereira (con agente de código).
- Resultado: completada.
- PR: main (commit incremental).
- Requisitos: FR-OPS-004.
- Evidencia: implementada migración `migrations/0001_init.sql` exactamente como especifica `docs/11 §2` (12 tablas de negocio con PK UUIDv7, índices y checks normativos); migrador `MIGRATOR` y verificador `check_schema_version` en `gs-store`; subcomando `geostream migrate` con `--dry-run` e idempotencia estricta; verificación obligatoria de versión del esquema en `geostream serve` al arrancar; 11 pruebas de restricciones en `crates/gs-store/tests/constraints_test.rs` y 7 pruebas de CLI en `apps/geostream/tests/cli_test.rs` verdes contra PostgreSQL 18 real; `cargo deny check` y `geostream-tools all` verdes.

### 2026-10-12 — T-F1-08 — Pruebas de propiedades (secuencias, capacidades) y cobertura >= 85 % de gs-domain
- Autor/agente: Angel Nereira (con agente de código).
- Resultado: completada.
- PR: main (commit incremental).
- Requisitos: NFR-MAINT-002.
- Evidencia: implementada suite `tests/property_tests.rs` con 8 propiedades `proptest` (modelo de referencia CAS de secuencias, invariantes de no escalada y mutación de capacidades INV-MEM-1..3, límites de coordenadas y rechazo de Null Island, normalización de códigos Crockford Base32, roundtrip de codec de IDs tipados, y expiración perezosa de sesiones INV-SES-1..3); cobertura de líneas en `gs-domain` verificada con `cargo llvm-cov` en 90.76 % (1738/1915 líneas); todos los criterios de salida de F1 cumplidos y Fase F1 formalmente cerrada.

### 2026-10-12 — T-F1-07 — Catálogo canónico de errores de dominio y mapeo a ErrorCode en gs-domain
- Autor/agente: Angel Nereira (con agente de código).
- Resultado: completada.
- PR: main (commit incremental).
- Requisitos: FR-API-001 (base).
- Evidencia: implementado catálogo de los 25 códigos de error `ErrorCode` (`docs/09 §6`, `common.proto`), mapeo HTTP, reintentos, mensaje seguro estándar, rasgo `AsErrorCode`, enum unificado `DomainError` y suite de pruebas `tests/error_mapping_test.rs`; `cargo test -p gs-domain` (43 tests) verde; `cargo deny check` y `geostream-tools -- all` verdes.

### 2026-10-12 — T-F1-06 — Carga y validación de configuración y políticas de tenant en gs-config
- Autor/agente: Angel Nereira (con agente de código).
- Resultado: completada.
- PR: main (commit incremental).
- Requisitos: FR-OPS-003, FR-ADM-002.
- Evidencia: implementados `ServerConfig`, `TenantPolicy`, `SecretString`, `DataEncryptionKey`, cargador y validador por entorno (`dev`, `test`, `prod`), restricciones y rechazo de secretos de ejemplo en producción; 16 pruebas automatizadas (`tests/server_config_test.rs` y `tests/policy_test.rs`); `cargo test -p gs-config` verde; `cargo deny check` y `geostream-tools -- all` verdes.

### 2026-10-12 — T-F1-05 — Generación y normalización de códigos de invitación en gs-domain
- Autor/agente: Angel Nereira (con agente de código).
- Resultado: completada.
- PR: main (commit incremental).
- Requisitos: FR-INV-001 (base).
- Evidencia: suite `tests/invitation_vectors_test.rs` con 100 % de vectores `contracts/test-vectors/invitation_codes.json` validados; generación verificada con longitud y formato estándar `XXXX-XXXX-XXXX-XXXX`; hash SHA-256; `cargo test -p gs-domain` (38 tests) verde; `cargo deny check` y `geostream-tools -- all` verdes.

### 2026-10-12 — T-F1-04 — Sesión, membresía, capacidades, compartición y reglas INV-* en gs-domain
- Autor/agente: Angel Nereira (con agente de código).
- Resultado: completada.
- PR: main (commit incremental).
- Requisitos: FR-SES-006, FR-SES-007, FR-MEM-003, FR-MEM-007, FR-SHR-001, FR-SHR-002, FR-SHR-004.
- Evidencia: Implementados `gs-domain::session` (`Session`, `SessionMode`, `SessionStatus`, `ClosedReason`, `Actor`) y `gs-domain::membership` (`Membership`, `MemberRole`, `MemberStatusKind`, `Capabilities`, `SharingTransition`, `Connectivity`, `MemberStatusView`, `OwnerDepartureOutcome`); reglas y máquinas de estado con 19 pruebas de integración en `crates/gs-domain/tests/invariants_test.rs` cubriendo explícitamente `INV-SES-1..3`, `INV-MEM-1..7`, `INV-SHR-1..4`, `INV-PRS-1`, modos de sesión `FR-SES-006`, salida de propietario `FR-SES-007` y readmisión `FR-MEM-007`.
- Notas: Lógica de dominio pura sin I/O; estricto cumplimiento de privacidad donde `sharing=false` nunca revela conectividad ni coordenadas.

### 2026-10-12 — T-F1-03 — Modelo de ordenación de secuencias y lotes en gs-domain
- Autor/agente: Angel Nereira (con agente de código).
- Resultado: completada.
- PR: main (commit incremental).
- Requisitos: FR-LOC-003.
- Evidencia: Implementados en `gs-domain::location` el almacén de secuencias `InMemorySequenceStore`, la semántica CAS atómica `CasResult` (Accepted, Duplicate, Stale), los errores de precondiciones de lote `BatchPreconditionError` (ValidationFailed, PayloadTooLarge), el resultado de lote `BatchEvaluationResult` y la función normativa `process_sample_batch` según docs/09 §4 y docs/08 §6.4. Suite de pruebas de integración parametrizada en `crates/gs-domain/tests/location_vectors_test.rs` validando los 9 lotes normativos `LOC-B-*` de `contracts/test-vectors/location_samples.json`.
- Notas: Cumple ordenación monotónica, deduplicación atómica, descarte de secuencias obsoletas y actualización de secuencia de snapshot.

### 2026-10-12 — T-F1-02 — Tipos de ubicación redactados y validate_sample puro en gs-domain
- Autor/agente: Angel Nereira (con agente de código).
- Resultado: completada.
- PR: main (commit incremental).
- Requisitos: FR-LOC-001, FR-LOC-002, FR-LOC-004, FR-LOC-006.
- Evidencia: Implementados en `gs-domain::location` los tipos de ubicación `Position` (con coordenadas privadas y `Debug`/`Display` redactados `Position { <redacted> }`), `LocationSource`, `LocationKind`, `MotionActivity`, `LocationQuality`, `LocationSample`, `AcceptedLocation`, `SampleStatus`, `SampleResult`, `LocationPolicy` y la función pura `validate_sample` (evaluando las 16 reglas en orden estricto de docs/09 §3.1) y `evaluate_sample` (reglas 1–16, antigüedad y orden de secuencia). Suite de pruebas de integración parametrizada en `crates/gs-domain/tests/location_vectors_test.rs` validando los 49 casos de `contracts/test-vectors/location_samples.json` (válidos e inválidos `LOC-V-*` y `LOC-I-*`) y la redacción de coordenadas en logs.
- Notas: Pureza total sin I/O, sin tokio; cero coordenadas en salidas `Debug`/`Display`.

### 2026-10-12 — T-F1-01 — IDs tipados con prefijo y codec en gs-domain
- Autor/agente: Angel Nereira (con agente de código).
- Resultado: completada.
- PR: main (commit incremental).
- Requisitos: FR-API-006 (base).
- Evidencia: Implementados los 14 IDs de entidad (`TenantId`, `ApplicationId`, `ApiKeyId`, `SubjectId`, `DeviceId`, `SessionId`, `MemberId`, `InvitationId`, `WebhookEndpointId`, `EventId`, `GeofenceId`, `ConnectionId`, `RequestId`, `AuditId`) basados en UUIDv7, `ExternalSubjectId` e `InstallationId` (UUIDv4 canónico); 5 pruebas de integración en `crates/gs-domain/tests/ids_vector_test.rs` validando todos los vectores de `contracts/test-vectors/ids.json` (válidos e inválidos).
- Notas: Tipos con Display, FromStr, Serialize, Deserialize y validaciones estrictas.

### 2026-10-12 — T-F0-08 — Verificación de nombres de paquetes y cierre de Fase F0
- Autor/agente: Angel Nereira (con agente de código).
- Resultado: completada.
- PR: main (commit incremental).
- Requisitos: — (preparación de publicación y cierre de bootstrap).
- Evidencia: Consultas a APIs de crates.io (`geostream`, `gs-protocol`, `geostream-sdk`, `geostream-server`) y pub.dev (`geostream`, `geostream_flutter`) confirmando 100% de disponibilidad; recordatorio de ADR-0019 (licencia) registrado; criterios de salida de Fase F0 satisfechos en su totalidad.
- Notas: Se da por cerrada la fase F0 y se activa formalmente la fase F1 (Núcleo de dominio).

### 2026-10-12 — T-F0-07 — docker-compose.dev.yml y .env.example
- Autor/agente: Angel Nereira (con agente de código).
- Resultado: completada.
- PR: main (commit incremental).
- Requisitos: — (infraestructura de desarrollo local).
- Evidencia: `deploy/docker-compose.dev.yml` validado con `docker compose config` (PostgreSQL 16 + Valkey 7.2 con healthchecks); `deploy/.env.example` con variables de entorno según `17_CONFIGURATION.md`.
- Notas: Entorno listo para desarrollo local y pruebas de integración.

### 2026-10-12 — T-F0-06 — Proceso de release y changelog
- Autor/agente: Angel Nereira (con agente de código).
- Resultado: completada.
- PR: main (commit incremental).
- Requisitos: NFR-MAINT-001.
- Evidencia: Plantilla de release creada en `docs/templates/RELEASE_TEMPLATE.md`; validador `tools/check_changelog.py` implementado y verificado en `tools/run_all_checks.py`.
- Notas: Cubre SemVer, política de deprecación (≥ 6 meses), checklists de seguridad (10 §11), QA y pasos de publicación.

### 2026-10-12 — T-F0-05 — CI etapas 1, 2, 3 y 5 y checks de consistencia en tools/
- Autor/agente: Angel Nereira (con agente de código).
- Resultado: completada.
- PR: main (commit incremental).
- Requisitos: NFR-DOC-001, NFR-SEC-006.
- Evidencia: Etapas 1, 2, 3, 4 y 5 de CI pasando localmente; `tools/run_all_checks.py` verde con 7 verificadores de consistencia documental y de dependencias (`check_json_validity.py`, `check_crate_deps.py`, `check_error_catalog.py`, `check_config_keys.py`, `check_requirements.py`, `check_auth_matrix.py`, `check_markdown_links.py`).
- Notas: Se amplió `.github/workflows/ci.yml` cubriendo todas las etapas aplicables.

### 2026-10-12 — T-F0-04 — gs-protocol con build.rs y prueba de ida y vuelta
- Autor/agente: Angel Nereira (con agente de código).
- Resultado: completada.
- PR: #4 (fusionado en `main`)
- Requisitos: — (verificación de contratos de stream).
- Evidencia: local, rustc 1.99.0 — `cargo test -p gs-protocol` (1 prueba de ida y vuelta completa que valida los 24 mensajes de `stream_messages.json`: JSON → Protobuf → Binario → Protobuf → JSON idéntico). `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo deny check`, `cargo audit`, `buf lint contracts/proto` limpios.
- Notas: `build.rs` compila contratos Protobuf con `prost-build` y `pbjson-build` usando `protoc-bin-vendored`. Tipos de timestamp mapeados a `pbjson_types::Timestamp`.

### 2026-10-12 — T-F0-03 — Binario geostream con subcomandos
- Autor/agente: Angel Nereira (con agente de código).
- Resultado: completada.
- PR: #3 (fusionado en `main`)
- Requisitos: FR-OPS-001.
- Evidencia: local, rustc 1.99.0 — `cargo test -p geostream` (5 pruebas, 0 fallos: `test_version_subcommand`, `test_migrate_stub`, `test_admin_stub`, `test_serve_roles_validation`, `test_serve_health_live`). `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo deny check`, `cargo audit` limpios.
- Notas: `build.rs` embebe el commit corto de git con fallback a `unknown`. `serve` implementa listener HTTP con endpoint `/health/live` y apagado ordenado ante SIGINT/SIGTERM. `migrate` y `admin` devuelven mensajes claros de no implementados (programados para F2).

### 2026-10-12 — T-F0-02 — Dependencias fijadas, cargo deny y cargo audit
- Autor/agente: Angel Nereira (con agente de código).
- Resultado: completada (resuelta con ADR-0020 y excepciones en `deny.toml`).
- PR: — (rama `chore/T-F0-02-dependencies-deny`)
- Requisitos: NFR-SEC-006.
- Evidencia: local, rustc 1.99.0, cargo-deny 0.20.2, cargo-audit 0.22.2 — `cargo deny check`: advisories, bans, licenses (con excepciones de ADR-0020) y sources ok. `cargo audit`: 558 paquetes, sin vulnerabilidades. `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings` y `cargo test --workspace --all-features` sin errores (0 pruebas). Workflow `.github/workflows/ci.yml` configurado con jobs de seguridad.
- Notas: versiones tomadas de crates.io el 2026-10-12. `dashmap` en 6.2.1. `testcontainers` en 0.27. `jsonwebtoken`, `sqlx` y `reqwest` usan `aws-lc-rs`. Cuatro dependencias transitivas permisivas aprobadas por ADR-0020 (`xxhash-rust` BSL-1.0, `webpki-root-certs` CDLA-Permissive-2.0, `webpki-roots` CDLA-Permissive-2.0, `borrow-or-share` MIT-0) tras consulta con el propietario (DR-20261012-01).

### 2026-10-12 — T-F0-01 — Workspace Cargo y toolchain
- Autor/agente: Angel Nereira (con agente de código).
- Resultado: completada.
- PR: #1 (fusionado en `main`)
- Requisitos: FR-OPS-001 (base; los subcomandos llegan en T-F0-03).
- Evidencia: local, rustc 1.99.0 — `cargo fmt --all --check` sin cambios; `cargo clippy --workspace --all-targets --all-features -- -D warnings` sin avisos; `cargo test --workspace` 0 pruebas, 0 fallos; `cargo metadata` lista 17 paquetes con las dependencias de `04` §4.1.
- Notas: `gs-testkit`, `gs-devclient` y `gs-loadgen` sin dependencias internas (`04` §4.1 no las define). `.editorconfig` y `.gitignore` revisados sin cambios. Versión de paquetes `0.0.0` y `publish = false` hasta ADR-0019.

### 2026-10-11 — Paquete documental v0.2.0
- Autor: diseño inicial.
- Resultado: documentación normativa, ADR, contratos (`buf lint` y Redocly sin errores), vectores y escenarios listos.
- Siguiente paso: T-F0-01.
