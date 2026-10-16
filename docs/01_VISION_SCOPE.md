# 01 — Visión, alcance y casos de uso

**ID:** GS-VIS-001 · **Versión:** 0.3.0

## 1. Resumen

GeoStream es un **motor de ubicación y movilidad** en tiempo real e histórico sobre el que otras aplicaciones y sistemas construyen experiencias de reparto, ride-hailing (tipo Uber o InDrive), gestión de flotas, rastreo de activos y localización de dispositivos sin internet, con privacidad por defecto. El motor vive en **dos mitades que hablan entre sí y comparten el mismo núcleo en Rust** ([ADR-0021](adr/ADR-0021-location-and-mobility-engine.md), [ADR-0022](adr/ADR-0022-shared-rust-client-core.md)):

- un **servidor** en Rust para Linux (binario único con roles `api`, `stream`, `worker` y `analytics`) que acepta y distribuye muestras, estima trayectorias en streaming (`gs-track`), indexa en vivo lo que se mueve (`gs-mobility`), responde consultas espaciales y de movilidad, guarda historial opt-in y calcula analítica (`gs-analytics`);
- un **núcleo de dispositivo** en Rust (`gs-client-core`, sans-IO + `gs-client-runtime`) con bindings idiomáticos para **Kotlin (Android), Swift (iOS), Dart (Flutter), TypeScript (web y Node), Go y Ruby** (`26_CORE_AND_BINDINGS.md`), que decide cuándo y cómo muestrear para ahorrar batería, limpia la señal local, publica por cambio significativo, predice visualmente entre muestras, guarda en cola cifrada sin conexión y hace de relevo para tags BLE sin internet;
- un **plano de control y movilidad REST** (sesiones, pools de disponibilidad, cercanos, anillos de expansión, densidad anónima, ETA, asignación por lotes, trazas, viajes, analítica, tags y webhooks);
- un **protocolo de streaming** sobre WebSocket seguro, versionado y definido en Protobuf;
- **adaptadores de mapas y Provider Gateway** opcionales y desacoplados con manifiestos declarativos y BYOK (`13_MAPS_PROVIDERS.md`, [ADR-0028](adr/ADR-0028-server-provider-gateway-byok.md));
- una **red de búsqueda Edge (GeoStream Beacon)** cifrada de extremo a extremo para tags BLE sin internet e ingesta desde LPWAN y rastreadores (`25_EDGE_DEVICES.md`, [ADR-0029](adr/ADR-0029-geostream-beacon-ble-network-and-dult.md)).

GeoStream **no es** un proveedor cartográfico ni una plataforma de negocio: pedidos, tarifas, pagos, políticas de despacho, pujas e incentivos o sanciones antifraude pertenecen al integrador. GeoStream le proporciona el motor y las herramientas matemáticas/espaciales para decidir.

## 2. Problema

Cada producto que necesita ubicación y movilidad en tiempo real (transporte, delivery, logística, seguridad, familia, eventos, IoT) reconstruye la misma infraestructura crítica: captura móvil con permisos, actividad y batería, filtrado de ruido GNSS y anti-deriva en paradas, canal en tiempo real con autorización y revocación inmediata, índice espacial de conductores disponibles, búsqueda por anillos o ETA, asignación por lotes, historial con consentimiento, relevo de dispositivos sin red y paridad entre múltiples lenguajes. Construirlo a medida suele resultar frágil, costoso e inconsistente entre plataformas.

## 3. Propuesta de valor

1. **Un núcleo Rust, dos mitades y seis lenguajes**: los algoritmos geoespaciales, el filtro de trayectoria y el protocolo se escriben una vez en Rust y se verifican con los mismos vectores dorados en servidor y clientes.
2. **Privacidad por diseño, también con historial**: compartir está apagado por defecto; el historial es opt-in por política del tenant, modo de sesión y consentimiento con propósito del subject (`HISTORY`, `ANALYTICS`); los agregados exigen k-anonimato y los tags BLE se cifran de extremo a extremo.
3. **Calidad de señal sin sacrificar la verdad medida**: el servidor nunca altera la muestra `MEASURED`; adjunta aparte la estimación (`FILTERED` o `MAP_MATCHED`), banderas de plausibilidad, odómetro y eventos de parada.
4. **Primitivas completas de movilidad**: pools de disponibilidad, búsqueda de cercanos (`nearby`), anillos crecientes (`rings`), densidad aproximada anónima (`density`), ETA por niveles y asignación óptima (Hungarian y flujo de coste mínimo).
5. **Despliegue modular**: la instalación mínima sigue siendo «un binario + PostgreSQL»; H3, proveedores, grafo vial, historial y edge se activan por política o *feature*.

## 4. Objetivos (G)

- **G-1** Compartir y consumir ubicación en tiempo real entre participantes autorizados, en sesiones 1-a-1, 1-a-N, N-a-N y `POOL`.
- **G-2** Control explícito y revocable por parte de cada persona sobre si comparte su ubicación y con qué propósitos (`LIVE`, `HISTORY`, `ANALYTICS`).
- **G-3** Revocación efectiva de lectura y publicación sin esperar a que expiren credenciales.
- **G-4** Corrección ante pérdida de red, duplicados, retrasos, reconexiones y relojes desfasados.
- **G-5** Núcleo de cliente compartido con SDKs y bindings tipados para Kotlin, Swift, Dart (Flutter), TypeScript (web y Node), Go y Ruby.
- **G-6** Integración con sistemas terceros: tokens server-to-server, webhooks, stream del lado servidor, ingesta HTTP e ingesta LPWAN.
- **G-7** Escalado horizontal sin cambiar el protocolo público.
- **G-8** Operación observable sin exponer coordenadas ni identificadores de alta cardinalidad.
- **G-9** Mapas, geocodificación, rutas, matrices y map-matching como adaptadores reemplazables con manifiesto y Provider Gateway BYOK, complementados por una capa aprendida con datos propios.
- **G-10** Geocercas con eventos de entrada, salida, permanencia (`DWELL`) y proximidad dinámica entre miembros.
- **G-11** Calidad de señal en streaming: puerta de plausibilidad, filtro de Kalman CV, anti-deriva en reposo, odómetro en vivo y detección de paradas (`22_TRACKING_ENGINE.md`).
- **G-12** Eficiencia de batería en el dispositivo: muestreo adaptativo por actividad e intención, publicación por cambio significativo, despertar por región del SO, política remota (`SamplingPolicy`) y predicción visual acotada.
- **G-13** Herramientas de movilidad en tiempo real: pools con índice espacial vivo (H3 en memoria y Valkey), búsqueda `nearby` por distancia o ETA, anillos de expansión `rings`, `density` anónima y asignación por lotes (`23_MOBILITY.md`).
- **G-14** Historial opt-in y analítica de movilidad con k-anonimato: trazas, viajes compactados (`gsp1`), mapas de calor, matrices origen-destino, lugares aprendidos y perfiles de velocidad (`24_HISTORY_ANALYTICS.md`).
- **G-15** Localización de dispositivos sin internet mediante la red BLE cifrada E2E GeoStream Beacon (con protecciones DULT) e ingesta LPWAN (`25_EDGE_DEVICES.md`).

## 5. No objetivos

- **NG-1** Reemplazar proveedores cartográficos (Google, Mapbox, HERE, OSM).
- **NG-2** Garantizar precisión absoluta: depende del dispositivo, entorno y sistema operativo.
- **NG-3** Garantizar ejecución indefinida en segundo plano: Android e iOS lo restringen.
- **NG-4** No implementa lógica de negocio de movilidad: pedidos, tarifas, pagos, políticas de despacho, incentivos ni sanciones antifraude; sí ofrece herramientas de proximidad, ETA, asignación como cálculo, rutas vía proveedor y map-matching.
- **NG-5** Historial sin consentimiento: el historial es estrictamente opt-in por política del tenant, modo de sesión y consentimiento con propósito del subject ([ADR-0024](adr/ADR-0024-opt-in-history-and-k-anonymous-analytics.md)).
- **NG-6** Exigir infraestructura pesada en despliegues básicos: una instalación mínima no requiere H3, grafo vial, proveedores, historial ni edge; todo se activa por política o *feature*.
- **NG-7** Prometer latencia, capacidad, precisión o ahorro de batería sin medición en un entorno definido.
- **NG-8** Acceso anónimo a ubicaciones de individuos. Toda lectura requiere un principal autenticado y autorizado, y todo agregado exige k-anonimato.

## 6. Actores

| Actor | Descripción |
|---|---|
| **Integrador** | Equipo que integra GeoStream en su producto (backend + apps móviles/web). |
| **Backend del integrador** | Servicio del integrador que autentica a sus usuarios, gestiona su lógica de negocio y consume la API/stream de GeoStream con una API key. |
| **Subject** | Persona o entidad del integrador identificada por `external_subject_id`. El consentimiento (`sharing` y `consent_purposes`) pertenece exclusivamente al subject. |
| **Dispositivo** | Instalación concreta de una app en un teléfono, navegador, servidor o equipo, o un rastreador asociado. Pertenece a un subject. |
| **Tag (Edge)** | Accesorio BLE sin conexión a internet registrado por un subject dueño, cuya clave `EIK` nunca conoce el servidor. |
| **Buscador (Finder)** | Dispositivo con el SDK que actúa como relevo anónimo de anuncios BLE cifrados en la red GeoStream Beacon. |
| **Propietario de sesión** | Subject que crea la sesión (si la crea un dispositivo). |
| **Miembro** | Subject con membresía activa en una sesión, con capacidades `publish`, `read`, `invite`, `manage` y, en pools, `availability` y `attributes`. |
| **Aplicación (principal)** | Credencial server-to-server del integrador dentro de un tenant. |
| **Operador** | Quien despliega y opera GeoStream. Observa salud y métricas sin acceso a coordenadas. |
| **Administrador de tenant** | Gestiona tenants, aplicaciones, API keys y políticas mediante la CLI admin. |

## 7. Casos de uso

Cada caso se convierte en uno o más escenarios ejecutables (`contracts/scenarios/`).

### UC-01 — Seguimiento de un viaje (ride-hailing)
El backend del integrador crea una sesión `ONE_TO_ONE` al asignar conductor y pasajero, agrega ambos subjects como miembros, y los dispositivos activan compartir. Ambos ven la ubicación del otro con estimación filtrada (`FILTERED`), predicción visual suave en el mapa (`PREDICTED`) y odómetro en vivo. Al terminar el viaje el backend cierra la sesión: se borran snapshots y terminan las suscripciones.

### UC-02 — Seguimiento de un pedido (delivery)
Sesión `ONE_TO_MANY`: el repartidor publica con muestreo adaptativo y despertar por región en paradas; el cliente (y el backend con `locations:read`) leen y reciben eventos `TrackEvent` cuando inicia o termina una parada.

### UC-03 — Compartir entre familia o equipo
Un dispositivo crea una sesión `MANY_TO_MANY` de larga duración (dentro de la política del tenant) e invita a otros con un código. Cada miembro decide individualmente si comparte. Dejar de compartir no saca al miembro de la sesión.

### UC-04 — Flota
Sesión propiedad de la aplicación con muchos publicadores (conductores con `publish`, sin `read`) y el backend como consumidor de stream del lado servidor. Los conductores no ven a los demás.

### UC-05 — Evento temporal
Sesión con expiración corta; los asistentes se unen por invitación multiuso; la sesión expira sola.

### UC-06 — Consulta de última ubicación autorizada
Un miembro con `read` (o el backend con `locations:read`) consulta el snapshot REST de una sesión, incluyendo la estimación del motor de trayectoria si está activa.

### UC-07 — Llegada, permanencia y proximidad (geocercas)
Una geocerca asociada a la sesión de un pedido dispara `geofence.entered` al llegar, `geofence.dwell` tras permanecer `dwell_seconds` en el restaurante, o un evento de proximidad dinámica cuando el conductor entra en el radio anclado al pasajero.

### UC-08 — Dispositivo IoT o gateway
Un equipo sin app móvil publica por ingesta HTTP en lotes con un token de dispositivo (`POST /v1/ingest/locations`), beneficiándose del filtro de trayectoria con control optimista (`track_cas.lua`).

### UC-09 — Pool de conductores disponibles y búsqueda de cercanos
La aplicación crea una sesión `POOL` (`pools:manage`). Los conductores se ponen en línea activando `sharing=true` y `availability=AVAILABLE`. Cuando entra una solicitud, el backend invoca `POST /v1/pools/{id}/nearby` con `rank=eta` para obtener los mejores candidatos o `POST /v1/tools/assignment` para asignar un lote de pedidos de forma óptima (`SCN-016`, `SCN-019`). Mientras tanto, la app del pasajero consulta `GET /v1/pools/{id}/density` (`pools:density`) para mostrar marcadores aproximados y anónimos alrededor (`SCN-017`).

### UC-10 — Ofertas progresivas por anillos (estilo InDrive)
Ante una solicitud de viaje con puja, el backend consulta `POST /v1/pools/{id}/rings` sobre el pool activo y recibe candidatos agrupados en anillos H3 concéntricos crecientes para difundir la oferta progresivamente (`SCN-017`).

### UC-11 — Historial de viajes y analítica de flota
En una sesión con `history_mode = TRACE_AND_ANALYTICS` donde el conductor otorgó consentimiento `HISTORY` y `ANALYTICS`, el motor persiste la traza (incluyendo el backfill `buffered` tras pasar por zonas sin señal, `SCN-021`), segmenta el viaje al cerrar con su polilínea compacta `gsp1` y alimenta mapas de calor k-anónimos (`SCN-020`, `SCN-023`), perfiles de velocidad y lugares aprendidos (`24_HISTORY_ANALYTICS.md`).

### UC-12 — Localización de tag BLE sin internet (GeoStream Beacon)
Un operador registra un tag BLE en un activo o paquete (`E-BLE`) conservando el `EIK` en su dispositivo dueño. Cuando cualquier teléfono de la flota con el SDK pasa cerca del tag, escucha su `EID` rotatorio, cifra su posición con la clave pública anunciada (`ECDH P-256 + AES-256-GCM`) y envía un reporte anónimo a `POST /v1/edge/reports`. El dueño consulta `POST /v1/edge/reports:lookup`, descifra localmente y obtiene la posición fusionada sin que el servidor haya podido leer las coordenadas (`SCN-025`).

### UC-13 — Ingesta desde rastreador LPWAN
Un rastreador GPS LoRaWAN en un contenedor envía tramas por radio a un gateway; el network server invoca el webhook autenticado por HMAC `POST /v1/ingest/lpwan/{integration_id}`, donde `gs-edge` decodifica el payload binario y lo inyecta en el pipeline normal de validación y trayectoria (`25_EDGE_DEVICES.md`).

## 8. Principios de diseño

Principios base:
- **P-1 Privacidad por defecto.** Compartir apagado al unirse; sin historial salvo triple llave activa; snapshot efímero con TTL; borrado síncrono al dejar de compartir; coordenadas nunca en logs.
- **P-2 La identidad sale de las credenciales.** Nunca de IDs del payload.
- **P-3 Ubicación ≠ mapa.** El núcleo funciona sin ningún proveedor de mapas.
- **P-4 Semántica latest-state en vivo.** La ubicación en vivo es estado reciente best-effort; los eventos de control son confiables e idempotentes.
- **P-5 Simplicidad operativa.** Monolito modular, un binario, dependencias mínimas y auditadas.
- **P-6 Afirmaciones medidas.** Distinguir objetivo, resultado medido y SLA.
- **P-7 La documentación gobierna.** Ver `00_INDEX.md`.

Principios del motor ([ADR-0021](adr/ADR-0021-location-and-mobility-engine.md)):
- **P-E1 · Un núcleo, muchos anfitriones.** Protocolo, algoritmos y validación se escriben una vez en Rust; cada lenguaje aporta solo lo específico de la plataforma.
- **P-E2 · Estado acotado.** Todo algoritmo del camino caliente es *online*: estado O(1) por entidad y cero asignaciones de memoria por muestra.
- **P-E3 · La medición es sagrada.** El servidor nunca modifica ni descarta una muestra `MEASURED` aceptada (`FR-LOC-006`); lo derivado viaja aparte y etiquetado con su `LocationKind`.
- **P-E4 · El camino caliente no espera.** Nada nuevo bloquea la aceptación ni el fan-out; lo pesado va por canales acotados con descarte medido o en el rol `analytics`.
- **P-E5 · Puertos para todo lo externo.** Proveedores, índices, almacenamiento, transporte y plataforma son traits con más de una implementación.
- **P-E6 · Determinismo verificable.** Los mismos vectores dorados de `contracts/test-vectors/` se ejecutan en Rust y en cada binding con tolerancias explícitas.
- **P-E7 · Opcional por diseño.** Cada capacidad se activa por política del tenant o por *feature*.
- **P-E8 · Ubicación ≠ mapa.** El proveedor dibuja y rutea; el motor sabe dónde está cada cosa y qué significa.
- **P-E9 · Privacidad también con historial.** Propósito explícito, consentimiento del subject, retención acotada, k-anonimato en agregados y E2E para tags.
- **P-E10 · Linux primero en servidor, batería primero en dispositivo.**

## 9. Definición de "producto listo" (GA y versiones posteriores)

- **GeoStream v1.0 GA** se alcanza al cumplir todos los criterios de salida de **F0–F12**, **E0–E6** y **E9** (para TypeScript; Kotlin, Swift y Dart ya llegan en E3; Go y Ruby como SHOULD) en `21_ROADMAP.md`:
  - Todos los requisitos MUST con fase ≤ F12 y E1–E6 implementados y trazados a pruebas en verde.
  - Motor geoespacial (`gs-geo`) y de trayectoria (`gs-track`) activos y dentro del presupuesto `NFR-PERF-005` y `NFR-PERF-006`.
  - Núcleo de cliente Rust (`gs-client-core`) y SDKs para Android (Kotlin), iOS (Swift), Flutter (Dart) y Web/Node (TypeScript) pasando el 100 % de los vectores aplicables (`NFR-COMPAT-003`).
  - Pools de movilidad (`POOL`), índice vivo en memoria y Valkey, consultas `nearby`/`rings`/`density`, ETA por niveles, asignación por lotes y Provider Gateway BYOK operativos.
  - Historial opt-in particionado, cola `buffered` cifrada, backfill y analítica H3 con k-anonimato verificados, con revisión de DPIA completada (`T-E6-11`).
  - Webhooks, consumidor de stream, ingesta HTTP, adaptadores de mapas (MapLibre y Google Maps) y geocercas (incluyendo `DWELL` y proximidad dinámica) operativos.
  - Benchmarks reproducibles publicados y checklist de seguridad de liberación completo.
- **GeoStream v1.1 (Fase E7):** capa de mapa aprendida (perfiles de velocidad, corrección de ETA, lugares aprendidos por DBSCAN) y map-matching HMM/Viterbi integrado sobre grafo propio opcional.
- **GeoStream v1.2 (Fase E8):** red de búsqueda GeoStream Beacon (`gs-edge`, tras spike de hardware `T-E8-01` y revisión criptográfica externa `T-E8-02`) e ingesta LPWAN.

## 10. Evolución arquitectónica

Resumen de hitos documentales; detalle en `CHANGELOG.md` y `docs/adr/README.md`:

- **v0.2.0 (2026-10-11):** línea base normativa inicial (identidad con `Subject` y `Device`, tickets de stream de un uso, secuencia por instalación con *sequence floor*, caché de autorización con `mver` y bus, presencia minimizada, contratos Protobuf y OpenAPI 3.1).
- **v0.3.0 (2026-10-15, Addendum A-02):** evolución a motor de ubicación y movilidad con núcleo Rust compartido servidor/dispositivo (ADR-0021..0032), kernel geoespacial (`gs-geo`), motor de trayectoria (`gs-track`), muestreo adaptativo y predicción visual (`gs-client-core`), pools e índice vivo H3/Valkey (`gs-mobility`), Provider Gateway BYOK (`gs-providers`), historial opt-in y analítica k-anónima (`gs-analytics`), red BLE cifrada E2E (`gs-edge`) y bindings multilenguaje (`gs-ffi`).
