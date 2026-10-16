# 02 — Glosario canónico

**ID:** GS-GLO-001 · **Versión:** 0.3.0

Términos en orden alfabético. El término en `monoespaciado` es el identificador usado en código y contratos.

## 1. Términos de la línea base (v1)

| Término | Identificador | Definición |
|---|---|---|
| Ack de publicación | `PublishAck` | Respuesta del servidor a un `PublishLocation`, con el resultado de cada muestra. |
| API key | `gsk_…` | Credencial server-to-server de una aplicación. Formato en `06_IDENTITY_AUTH.md` §3. |
| Aplicación | `Application` / `app_…` | Cliente registrado dentro de un tenant (por ejemplo, "app de conductores"). Posee API keys y política de scopes de dispositivo. |
| Bus | `Bus` | Canal publicación/suscripción entre instancias para eventos efímeros y de control. Implementaciones: memoria (un nodo) y Redis/Valkey Pub/Sub. |
| Capacidad | `Capabilities` | Permisos de un miembro dentro de una sesión: `publish`, `read`, `invite`, `manage`. |
| Captured at | `captured_at` | Instante de captura de la muestra, expresado en el dominio de tiempo del servidor (corregido por el SDK). |
| Coalescencia | — | Reemplazar en la cola de salida una ubicación pendiente de un miembro por otra más reciente del mismo miembro. |
| Compartir | `sharing` | Estado de una membresía que indica que su subject ha activado la publicación de ubicación en esa sesión. Apagado por defecto. |
| Conectividad | `Connectivity` | `ONLINE` / `OFFLINE` / `UNSPECIFIED` (desconocida) del dispositivo publicador de un miembro que comparte. No se expone para quien no comparte. |
| Conexión | `connection_id` / `con_…` | Una conexión WebSocket autenticada. Un dispositivo tiene como máximo una activa. |
| Consumidor del lado servidor | — | Backend del integrador conectado al stream con un principal de aplicación y scope `locations:read`. |
| Dispositivo | `Device` / `dev_…` | Instalación de una app de una aplicación para un subject. Identidad = (aplicación, `installation_id`, subject). |
| Dispositivo publicador | `publisher_device_id` | El único dispositivo del subject autorizado a publicar en una membresía mientras comparte. |
| Época de autenticación del subject | `token_epoch` / claim `sep` | Contador por subject; incrementarlo invalida todos sus tokens. |
| Escenario | `SCN-NNN` | Secuencia ejecutable de pasos REST/stream con resultados esperados (`contracts/scenarios/`). |
| Geocerca | `Geofence` / `geo_…` | Región (círculo o polígono) cuya entrada/salida genera eventos (F11). |
| Idempotency key | `Idempotency-Key` | Cabecera que hace seguros los reintentos de POST. |
| Installation ID | `installation_id` | UUID v4 generado por el SDK en la primera ejecución y persistido junto con la secuencia. No es secreto. |
| Invitación | `Invitation` / `inv_…` | Autorización canjeable por código para unirse a una sesión con capacidades concedidas. |
| Latest-state | — | Semántica de entrega: interesa el estado más reciente; las ubicaciones antiguas pueden descartarse. |
| Membresía / Miembro | `Membership` / `mem_…` | Relación activa entre un subject y una sesión, con rol y capacidades. |
| Muestra de ubicación | `LocationSample` | Una medición (o estimación etiquetada) con `sequence`, `captured_at`, `position`, `quality`. |
| Política de tenant | `TenantPolicy` | Conjunto de límites configurables por tenant (`17_CONFIGURATION.md` §4). |
| Principal | `Principal` | Quién realiza una petición autenticada: `Application` (API key) o `Device` (token de dispositivo). |
| Propietario | `role = OWNER` | Miembro creador de una sesión creada por dispositivo. Tiene todas las capacidades. |
| Received at | `received_at` | Instante en que el servidor aceptó la muestra. Lo asigna siempre el servidor. |
| Revocación | — | Retirada de un permiso (membresía, token, dispositivo, tenant) con efecto en vivo acotado por NFR-SEC-002. |
| Scope | `scope` | Permiso de una credencial (API key o token) sobre familias de operaciones. Distinto de capacidad. |
| Secuencia | `sequence` | Entero monotónico por instalación de dispositivo, asignado por el SDK a cada muestra. |
| Sequence floor | `sequence_floor` | Mayor secuencia aceptada conocida por el servidor para un dispositivo; el SDK la usa para no retroceder. |
| Sesión | `Session` / `ses_…` | Contexto de compartición con modo, expiración, límites y miembros. |
| Snapshot | `LocationSnapshot` | Última ubicación aceptada de un miembro que comparte, retenida con TTL efímero. |
| Subject | `Subject` / `sub_…` | Persona o entidad del integrador (`external_subject_id`). Dueña del consentimiento. |
| Suscripción | `Subscribe` | Solicitud de una conexión para recibir eventos y ubicaciones de una sesión. |
| Tenant | `Tenant` / `ten_…` | Límite de aislamiento lógico y de políticas. Ningún dato cruza tenants. |
| Ticket de stream | `gst_…` | Credencial opaca de un solo uso y vida corta para abrir una conexión WebSocket. |
| Token de dispositivo | JWT EdDSA | Access token emitido por GeoStream para un dispositivo de un subject. |

## 2. Términos del motor de ubicación y movilidad (pista E)

| Término | Identificador | Definición |
|---|---|---|
| Agregado analítico | — | Resultado de una agregación sobre historial que **no** contiene series individuales y que suprime las unidades con menos de `analytics_k_anonymity` subjects distintos (`24` §6). |
| Anonimato k | `k` | Umbral de supresión de agregados; por defecto `analytics_k_anonymity = 5` (`24` §6). |
| Anti-deriva en reposo | ZUPT | Pseudo-medición de velocidad cero que se inyecta cuando el movimiento detectado cae por debajo de la puerta de movimiento (`22` §2.4). |
| Bandera de plausibilidad | `PlausibilityFlag` | Señal de bajo nivel (`EXCESSIVE_SPEED`, `OUTLIER_JUMP`, `LOW_ACCURACY`, `MOCK_FLAGGED`, `FROZEN_FIX`, `IMPLAUSIBLE_ACCEL`, `NETWORK_ONLY`) que acompaña a la estimación. GeoStream **no** decide fraude (D-10). |
| Buscador (finder) | — | Dispositivo o app que escucha tramas BLE del tag y reporta su sightings cifrados (`25` §2.1). |
| BYOK | — | *Bring Your Own Key*: la credencial del proveedor de mapas/rutas es **del tenant**, no de GeoStream; se cifra en reposo (`23` §8, `13` §8). |
| Capacidad sinIO (sans-IO) | — | Núcleo de lógica sin dependencias de E/S ni de plataforma; parametrizado por puertostraits implementados por el host (`26` §1.1). |
| Celda H3 | `h3_cell` | Índice jerárquico de celdas hexagonales. Es una **coordenada** a efectos de redacción: prohibido en logs, métricas y trazas (`10` §8.2). |
| Checkpoint | `checkpoint` | Punto de control del filtro: versión persistente del estado del motor de trayectoria, escrita cada `GS_TRACK_CHECKPOINT_INTERVAL_S` (`22` §2.1). |
| Cola sin conexión cifrada | — | Cola local del dispositivo, cifrada en reposo, que conserva muestras para entrega por lotes cuando no hay red (`22` §3.5, ADR-0025). |
| Conductor | — | Miembro **dispositivo** de una sesión `POOL`. Solo puede publicar; nunca ve a los demás conductores (`23` §1.1, INV-POOL-1). |
| Consensus de map-matching | — | Algoritmo Viterbi con retardo fijo (`GS_MM_FIXED_LAG`) que proyecta la trayectoria sobre un grafo vial (`24` §3.3). |
| DULT | — | Detección de rastreadores no deseados: estado *separado* en el tag y alerta en el cliente cuando se cumplen los umbrales de sightings, lugares distintos, distancia y duración (`25` §3). |
| EID | `EID` | Identificador **efímero** que el tag BLE anuncia; rota cada `edge_rotation_s` (`25` §2.3). |
| EIK | `EIK` | Clave de instalación del tag. **Solo** vive en el tag y en el dispositivo dueño; el servidor nunca la almacena (`25` §2.2, INV-EDGE-1). |
| Estimación | `LocationEstimate` | Posición derivada que acompaña a una muestra sin sustituirla. `kind ∈ {FILTERED, MAP_MATCHED}` (`22` §2.7, P-E3). |
| Exportación | `Export` / `exp_…` | Volcado asíncrono de historial o auditoría en NDJSON/CSV con URL de descarga de TTL corto (`24` §4). |
| Índice espacial | live index | Índice espacial en memoria de posiciones **actuales** de los miembros conectados (`23` §2). Se borra de forma síncrona al desconectar. |
| Grafo vial propio | `gsg` | Formato binario de grafo de rutas descargable por región (`GS_GRAPH_PATHS`); alternativa sin proveedor al map-matching en zonas sin cobertura (`23` §7, ADR-0032). |
| Groupa de muestreo | `SamplingRule` | Regla de la `SamplingPolicy` que asocia intervalo, precisión, velocidad y zona a una frecuencia de publicación (`22` §3.2). |
| Historial | `history_mode` / `LocationPoint` | Registro **opt-in** de puntos, particionado por `captured_at`. Exige triple llave: política del tenant, modo de sesión y consentimiento del subject (`24` §1). |
| Índice vivo | live index | Índice espacial en memoria de posiciones **actuales** de los miembros conectados (`23` §2). Se borra de forma síncrona al desconectar. |
| Integración LPWAN | `lpw_…` | Ingesta de posiciones de LoRaWAN/NB-IoT por webhook firmado HMAC con un decodificador por modelo (`25` §4). |
| Learned place | `LearnedPlace` / `plc_…` | Lugar recurrente descubierto por DBSCAN sobre centros de parada. Derivado, **anónimo** y k-anónimo (`24` §3). |
| Lugar (parada en un viaje) | `Stop` | Intervalo de permanencia con centroide, radio, duración y número de puntos (`22` §2.6, `24` §3). |
| Mapa de calor | `Heatmap` | Agregado k-anónimo de presencia, paradas o velocidad por celda H3 (`24` §4). |
| Map-matching | `MAP_MATCHED` | Proyección de una trayectoria sobre un grafo vial; etiqueta `kind` de la estimación (`24` §3.3). |
| Matriz origen-destino | OD matrix | Agregado k-anónimo de pares (origen, destino) en celdas H3 de resolución `GS_OD_RESOLUTION` (`24` §3.1). |
| Membresía en pool | `POOL` | Modo de sesión `POOL`: sesión de disponibilidad, no un concepto nuevo (`05` §4.2.1, `23` §1.1). |
| Muestreo adaptativo | — | Elección local de la frecuencia de publicación según actividad, precisión y velocidad (`22` §3.2). |
| Modo de historial | `HistoryMode` | `OFF` (por defecto), `TRACE` o `TRACE_AND_ANALYTICS`; atributo de sesión visible en `SessionInfo`. |
| Núcleo Rust compartido | `gs-client-core` | Lógica de cliente en Rust (muestreo, suavizado, cola, predicción) reutilizada por todos los SDK (`26` §1, ADR-0022). |
| Odómetro | `odometer` | Distancia acumulada estimada por el filtro, con puente de huecos y ruido proporcional (`22` §2.5). |
| Perfil de movilidad | `MobilityProfile` | Resumen k-anónimo de un sujeto o cohorte: tipo de día, ritmo y destinos recurrentes. Exige consentimiento `ANALYTICS` (`24` §4). |
| Perfil de velocidad | `SpeedProfile` | Percentiles de velocidad por celda H3 y franja horaria, con supresión k-anónima (`24` §3.1). |
| Playback de traza | `replay_trace` | Opción de `Subscribe` que reenvía la traza simplificada de una sesión activa con `delivery = REPLAY` (`08` §6.3, `24` §4). |
| Pool de disponibilidad | `POOL` / `SessionMode.POOL` | Ver «Membresía en pool». |
| Predicción visual | dead reckoning | Estimación **local y efímera** de la posición para dibujar con suavidad. Nunca se publica, nunca se persiste, nunca se registra (`08` §7.6, ADR-0030). |
| Propósito de consentimiento | `consent_purposes` | Subconjunto de `{HISTORY, ANALYTICS}` que **solo el propio subject** puede otorgar o retirar (`05` §7.2, INV-SHR-5). |
| Proveedor (gateway) | `ProviderGateway` | Fachada del servidor que delega ETA, ruta, map-matching o geocodificación en un proveedor externo usando la credencial BYOK del tenant (`23` §8). |
| Publicación por cambio significativo | — | Publicar cuando la posición se aleja, empeora la precisión, gira, cambia de velocidad o tras `publish_max_silence_s` (`22` §3.3). |
| Reporte Beacon | `EdgeReport` | Sightings cifrados de un tag, almacenados como blob opaco con TTL; el servidor no puede leerlos (`25` §2.6). |
| Replay | `REPLAY` | `DeliveryKind` de una actualización enviada por `replay_trace`, distinta de `LIVE` y `SNAPSHOT`. |
| Tag BLE | `Tag` / `tag_…` | Etiqueta de detección con identificador rotatorio y protección DULT (`25` §2). Solo se guardan metadatos, nunca la `EIK`. |
| Umbral de movimiento | — | Velocidad mínima (`0.5 m/s` por defecto) para considerar que hay movimiento y dejar de aplicar ZUPT (`22` §2.4). |
| Vector de prueba | `GEO-*`, `TRK-*`, `SMP-*`, `POOL-*`, `ETA-*`, `ASG-*`, `EDGE-*` | Par (entrada, salida esperada) usado para fijar el comportamiento del motor (`contracts/test-vectors/`). |
| Viaje | `Trip` / `trp_…` | Recorrido con inicio, fin, duración, distancia y polilínea `gsp1` simplificada; se deriva al cerrar la traza (`24` §3). |
| `Availability` de pool | `availability` | `AVAILABLE`, `BUSY` o `PAUSED`; solo en sesiones `POOL` y solo visible para principales de aplicación (`05` §8, INV-PRS-2). |
