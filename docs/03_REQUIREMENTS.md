# 03 — Requisitos funcionales y no funcionales

**ID:** GS-REQ-001 · **Versión:** 0.2.0

## 1. Cómo leer este documento

Cada requisito tiene el formato:

```
#### <ID> · <PRIORIDAD> · <FASE> — <Título>
- Descripción: qué debe hacer el sistema.
- Aceptación: criterio observable y verificable por una prueba automatizada (salvo que diga "manual").
- Ref: documentos o secciones que contienen el detalle normativo.
```

- Prioridades MUST / SHOULD / COULD: ver `00_INDEX.md` §4.1.
- La fase es la **fase en la que el requisito debe quedar cumplido** (ver `21_ROADMAP.md`).
- Los números (límites, TTL) se citan por nombre de parámetro; sus valores están en `17_CONFIGURATION.md`.
- Toda prueba que verifique un requisito DEBE incluir su ID en el nombre o en un comentario (`// covers: FR-SES-001`) para alimentar `TRACEABILITY.md`.

---

## 2. Requisitos funcionales

### 2.1 Identidad y autenticación (AUTH)

#### FR-AUTH-001 · MUST · F2 — API keys de aplicación
- Descripción: cada aplicación puede tener varias API keys con scopes. El secreto solo se muestra al crearse; se almacena su hash SHA-256. Las keys pueden revocarse y expirar.
- Aceptación: una key creada con la CLI autentica peticiones; tras revocarla, la misma petición devuelve 401 `UNAUTHENTICATED`; la base de datos nunca contiene el secreto en claro.
- Ref: `06_IDENTITY_AUTH.md` §3.

#### FR-AUTH-002 · MUST · F2 — Emisión de tokens de dispositivo
- Descripción: `POST /v1/tokens`, invocado por una aplicación con scope `tokens:mint`, crea o reutiliza el subject (`external_subject_id`) y el dispositivo (`installation_id`) y devuelve un JWT EdDSA con los claims definidos y TTL acotado.
- Aceptación: el token contiene exactamente los claims de `06` §4.2; `ttl_seconds` fuera de rango devuelve 422 `VALIDATION_FAILED`; dos llamadas con el mismo `installation_id` y subject devuelven el mismo `device_id`.
- Ref: `06` §4; OpenAPI `mintToken`.

#### FR-AUTH-003 · MUST · F2 — Verificación de tokens
- Descripción: toda petición con token de dispositivo verifica firma, `kid`, `iss`, `aud`, `nbf`, `exp` (con margen `GS_JWT_LEEWAY_S`), estado de tenant, aplicación, subject y dispositivo, y `sep` contra la época actual del subject.
- Aceptación: pruebas negativas para cada claim alterado, token expirado, dispositivo desactivado, subject con época incrementada y tenant deshabilitado; todas devuelven 401 o 403 con el código de `09` §6.
- Ref: `06` §4.3.

#### FR-AUTH-004 · MUST · F2 — Rotación de claves de firma
- Descripción: el servidor firma con la clave activa (`GS_JWT_ACTIVE_KID`) y verifica con cualquier clave del conjunto configurado.
- Aceptación: un token firmado con una clave anterior aún presente en el conjunto se verifica; tras retirarla del conjunto, se rechaza con 401.
- Ref: `06` §4.4.

#### FR-AUTH-005 · MUST · F3 — Revocación de tokens y desactivación de dispositivos
- Descripción: `POST /v1/subjects/{subject_id}/revoke-tokens` incrementa `token_epoch`; `DELETE /v1/devices/{device_id}` desactiva el dispositivo y detiene la compartición donde sea publicador.
- Aceptación: tras revocar, peticiones REST con tokens previos devuelven 401 y las conexiones de stream abiertas se cierran con 4401, ambas dentro del plazo NFR-SEC-002.
- Ref: `06` §7.

#### FR-AUTH-006 · MUST · F4 — Tickets de stream de un solo uso
- Descripción: `POST /v1/stream/tickets` emite un ticket opaco ligado al principal, con TTL `GS_TICKET_TTL_S`, canjeable una sola vez al abrir el WebSocket.
- Aceptación: un segundo uso del mismo ticket, o un uso tras su expiración, recibe HTTP 401 sin upgrade.
- Ref: `06` §5; `08` §3.

#### FR-AUTH-007 · MUST · F4 — Renovación de autenticación dentro de la conexión
- Descripción: el servidor envía `AuthExpiring` `GS_STREAM_AUTH_EXPIRING_LEAD_S` segundos antes de la expiración; el cliente responde `AuthRefresh` con un token nuevo del mismo dispositivo; si no lo hace, el servidor cierra con 4401 al expirar.
- Aceptación: escenario con TTL corto: con refresh la conexión sigue; sin refresh se cierra con 4401; un token de otro dispositivo en `AuthRefresh` produce `StreamError` `PERMISSION_DENIED` y cierre 4403.
- Ref: `08` §6.6.

#### FR-AUTH-008 · MUST · F2 — Scopes
- Descripción: cada operación exige un scope (`06` §6). Las API keys y los tokens solo obtienen scopes permitidos por la aplicación.
- Aceptación: matriz de pruebas: toda operación sin su scope devuelve 403 `INSUFFICIENT_SCOPE`.
- Ref: `06` §6; extensiones `x-gs-*` de OpenAPI.

#### FR-AUTH-009 · COULD · F15 — Tokens firmados por el integrador
- Descripción: alternativa en la que el integrador firma tokens con una clave pública registrada (JWKS).
- Aceptación: definida en un ADR antes de implementarse.
- Ref: ADR-0006 §Alternativas.

### 2.2 Administración y tenants (ADM)

#### FR-ADM-001 · MUST · F2 — CLI de administración
- Descripción: subcomandos `geostream admin` para crear, listar y deshabilitar tenants y aplicaciones; crear, listar y revocar API keys; generar claves de firma; ver y modificar políticas.
- Aceptación: pruebas de integración ejecutan cada subcomando contra PostgreSQL real; la salida de `key create` muestra el secreto una sola vez.
- Ref: `16_OPERATIONS.md` §3.

#### FR-ADM-002 · MUST · F2 — Políticas por tenant
- Descripción: cada tenant tiene una política con los parámetros de `17_CONFIGURATION.md` §4, validados contra sus rangos; los valores ausentes toman el default.
- Aceptación: establecer un valor fuera de rango falla con mensaje explícito; las operaciones respetan los límites de la política efectiva.
- Ref: `17` §4.

#### FR-ADM-003 · MUST · F3 — Deshabilitar tenant o aplicación
- Descripción: deshabilitar un tenant o una aplicación bloquea su acceso REST y cierra sus conexiones de stream.
- Aceptación: tras deshabilitar, REST devuelve 403 `ACCOUNT_DISABLED` y las conexiones abiertas se cierran con 4403, ambas dentro del plazo NFR-SEC-002.
- Ref: `06` §7.

#### FR-ADM-004 · COULD · F15 — API HTTP de administración
- Descripción: endpoints de administración para una consola SaaS.
- Aceptación: requiere ADR y modelo de amenazas propio.

### 2.3 API general (API)

#### FR-API-001 · MUST · F3 — Errores Problem Details
- Descripción: toda respuesta de error REST usa `application/problem+json` (RFC 9457) con `code` del catálogo de `09` §6 y `request_id`.
- Aceptación: pruebas de contrato validan todas las respuestas 4xx/5xx contra el esquema `Problem`.
- Ref: `07` §5; `09` §6.

#### FR-API-002 · MUST · F3 — Idempotencia
- Descripción: todo `POST` mutante acepta `Idempotency-Key`; misma key y mismo cuerpo devuelven la respuesta original; misma key con otro cuerpo devuelve 422 `IDEMPOTENCY_KEY_REUSED`.
- Aceptación: pruebas de reintento concurrente y secuencial para cada `POST` mutante.
- Ref: `07` §6.

#### FR-API-003 · MUST · F3 — Paginación por cursor
- Descripción: los listados usan `limit` y `cursor` opacos; responden `{ data, next_cursor }`.
- Aceptación: recorrer un listado de 120 elementos con `limit=50` devuelve 3 páginas sin duplicados ni omisiones.
- Ref: `07` §7.

#### FR-API-004 · MUST · F3 — Rate limit REST
- Descripción: límites por principal (`GS_REST_RATE_LIMIT_*`); al excederse se responde 429 `RATE_LIMITED` con `Retry-After`.
- Aceptación: prueba que supera el límite recibe 429 con `Retry-After` entero ≥ 1.
- Ref: `07` §8.

#### FR-API-005 · MUST · F3 — OpenAPI como fuente de verdad
- Descripción: `contracts/openapi/geostream.v1.yaml` define rutas, esquemas, scopes y principales; las respuestas reales se validan contra él.
- Aceptación: suite de conformidad valida request/response de todas las operaciones implementadas; CI falla ante divergencias.
- Ref: `07` §1; `18` §5.

#### FR-API-006 · MUST · F3 — No enumeración de recursos
- Descripción: si el principal no tiene ninguna relación con el recurso, se responde 404 `NOT_FOUND`; si tiene relación pero no la capacidad, 403 `PERMISSION_DENIED`.
- Aceptación: un no-miembro que consulta una sesión existente recibe la misma respuesta que para un ID inexistente.
- Ref: `06` §8.

### 2.4 Sesiones (SES)

#### FR-SES-001 · MUST · F3 — Crear sesión
- Descripción: un dispositivo con scope `sessions:create` o una aplicación con `sessions:write` crea una sesión con modo, expiración, límites y metadatos opcionales. Si la crea un dispositivo, su subject queda como propietario.
- Aceptación: 201 con el recurso `Session`; valores por defecto y topes de política aplicados; metadatos fuera de límites devuelven 422.
- Ref: `05` §4; `07` §9.2.

#### FR-SES-002 · MUST · F3 — Consultar y listar sesiones
- Descripción: obtener una sesión por ID y listar las sesiones del principal (dispositivo: donde su subject es miembro activo; aplicación: todas las del tenant con filtros).
- Aceptación: un dispositivo solo ve sesiones donde es miembro; filtros `status` y `external_subject_id` funcionan para aplicaciones.
- Ref: `07` §9.2.

#### FR-SES-003 · MUST · F3 — Actualizar sesión
- Descripción: `PATCH` permite extender `expires_at` dentro de la política, cambiar `max_members` (≥ miembros activos), `metadata`, `snapshot_ttl_seconds` y `stale_after_seconds`. Soporta `If-Match`.
- Aceptación: `If-Match` con versión obsoleta devuelve 412 `PRECONDITION_FAILED`; reducir `max_members` por debajo de los miembros activos devuelve 409 `CONFLICT`.
- Ref: `05` §4.

#### FR-SES-004 · MUST · F3 — Cerrar sesión
- Descripción: `POST /close` (idempotente) pasa la sesión a `CLOSED`, pone `sharing=false` en todas las membresías, borra snapshots y termina suscripciones con `SESSION_CLOSED`.
- Aceptación: tras cerrar, los snapshots no existen en el almacén efímero y los suscriptores reciben `SubscriptionEnded`; un segundo cierre devuelve 200 con el mismo estado.
- Ref: `05` §4.3.

#### FR-SES-005 · MUST · F3 — Expiración automática
- Descripción: las sesiones pasan a `EXPIRED` al alcanzar `expires_at`, por el worker (`GS_WORKER_EXPIRY_INTERVAL_S`) y por verificación perezosa en cada acceso, con los mismos efectos que el cierre.
- Aceptación: una sesión expirada nunca acepta publicaciones ni suscripciones, aunque el worker aún no haya corrido.
- Ref: `05` §4.3.

#### FR-SES-006 · MUST · F3 — Modos de sesión
- Descripción: `ONE_TO_ONE`, `ONE_TO_MANY`, `MANY_TO_MANY` con capacidades por defecto y límites de miembros según `05` §4.2.
- Aceptación: en `ONE_TO_ONE` un tercer miembro recibe 409 `SESSION_FULL`; en `ONE_TO_MANY` quien se une por invitación sin grants explícitos no tiene `publish`.
- Ref: `05` §4.2.

#### FR-SES-007 · MUST · F3 — Salida del propietario
- Descripción: si el propietario abandona o es revocado, la propiedad pasa al miembro activo más antiguo con `manage`; si no hay ninguno y la sesión fue creada por un dispositivo, la sesión se cierra con razón `OWNER_LEFT`.
- Aceptación: pruebas para ambas ramas.
- Ref: `05` §5.4.

### 2.5 Miembros (MEM)

#### FR-MEM-001 · MUST · F3 — Agregar miembros (aplicación)
- Descripción: una aplicación con `members:write` agrega un subject por `external_subject_id` (creándolo si no existe) con rol y capacidades.
- Aceptación: 201 con `Member`; agregar a un subject ya activo devuelve 409 `CONFLICT`.
- Ref: `05` §5.

#### FR-MEM-002 · MUST · F3 — Listar y consultar miembros
- Descripción: listar y consultar miembros con estado (`sharing`, `connectivity`, `last_location_at`). Requiere capacidad `read` o `manage`, o aplicación con `sessions:read`. Un miembro siempre puede consultarse a sí mismo (`me`).
- Aceptación: un miembro con solo `publish` recibe 403 al listar y 200 al consultar `me`.
- Ref: `05` §5; `06` §8.

#### FR-MEM-003 · MUST · F3 — Cambiar capacidades
- Descripción: `PATCH` de capacidades por un miembro con `manage` o la aplicación. Un miembro no puede conceder capacidades que no tiene. Las capacidades del propietario no se pueden reducir.
- Aceptación: pruebas de escalada negativa; quitar `publish` a quien comparte detiene su compartición y borra su snapshot.
- Ref: `05` §5.3.

#### FR-MEM-004 · MUST · F3 — Revocar miembro
- Descripción: `DELETE` de un miembro por quien tenga `manage` o la aplicación. Efecto en vivo: fin de suscripciones, rechazo de publicaciones, borrado de snapshot, evento a los demás.
- Aceptación: escenario SCN-002 pasa, incluyendo el plazo NFR-SEC-002.
- Ref: `05` §5; `08` §9.

#### FR-MEM-005 · MUST · F3 — Abandonar sesión
- Descripción: `POST /leave` por el propio miembro; mismos efectos que la revocación con razón `MEMBER_LEFT`.
- Aceptación: tras abandonar, el subject no recibe eventos de la sesión.
- Ref: `05` §5.

#### FR-MEM-006 · MUST · F3 — Límite de miembros
- Descripción: no se superan `max_members` de la sesión ni `max_members_cap` del tenant.
- Aceptación: altas concurrentes que exceden el límite: exactamente `max_members` tienen éxito; el resto recibe 409 `SESSION_FULL`.
- Ref: `05` §4; `11` §2 (bloqueo de fila).

#### FR-MEM-007 · MUST · F3 — Readmisión
- Descripción: un subject que abandonó puede volver por invitación o alta directa (nueva membresía, nuevo `member_id`). Un subject revocado solo vuelve por alta directa de la aplicación o de un miembro con `manage`; sus canjes de invitación fallan con `INVITATION_INVALID`.
- Aceptación: pruebas para ambos casos.
- Ref: `05` §5.5.

### 2.6 Invitaciones (INV)

#### FR-INV-001 · MUST · F3 — Crear invitación
- Descripción: un miembro con `invite` o la aplicación crea una invitación con `grants` ⊆ capacidades del emisor, `max_uses` y expiración dentro de la política. El código se devuelve una sola vez.
- Aceptación: grants por encima de las capacidades del emisor devuelven 403; el código no aparece en respuestas posteriores ni en la base de datos (solo su hash).
- Ref: `05` §6.

#### FR-INV-002 · MUST · F3 — Canjear invitación
- Descripción: `POST /v1/invitations/redeem` crea la membresía. Los fallos de canje (inexistente, expirada, agotada, revocada, subject revocado, sesión no activa) devuelven el mismo 404 `INVITATION_INVALID`; las únicas excepciones son 409 `CONFLICT` (el subject ya es miembro activo) y 409 `SESSION_FULL` (`05` §6). Rate limit por dispositivo `GS_INVITE_REDEEM_RPM`.
- Aceptación: SCN-007 pasa; los usos se cuentan de forma atómica bajo concurrencia.
- Ref: `05` §6.

#### FR-INV-003 · MUST · F3 — Listar y revocar invitaciones
- Descripción: listar (sin códigos) y revocar invitaciones por quien tenga `manage`, por su emisor o por la aplicación.
- Aceptación: una invitación revocada no puede canjearse.
- Ref: `07` §9.4.

#### FR-INV-004 · COULD · F15 — Aprobación de solicitudes de unión
- Descripción: invitaciones con `requires_approval` que crean solicitudes pendientes.
- Aceptación: requiere ADR.

### 2.7 Compartición y consentimiento (SHR)

#### FR-SHR-001 · MUST · F3 — Apagado por defecto
- Descripción: toda membresía nueva tiene `sharing=false`.
- Aceptación: ninguna ruta de alta (creación, alta directa, invitación) produce `sharing=true`.
- Ref: `05` §7.

#### FR-SHR-002 · MUST · F3 — Activación solo por el propio subject
- Descripción: solo un dispositivo del propio subject, con scope `sharing:write` y capacidad `publish`, puede poner `sharing=true` (`PUT …/members/me/sharing`). Ese dispositivo pasa a ser `publisher_device_id`.
- Aceptación: una aplicación o un manager que intente `sharing=true` sobre otro miembro recibe 403 `PERMISSION_DENIED`.
- Ref: `05` §7.

#### FR-SHR-003 · MUST · F3 — Detener compartición
- Descripción: el propio subject, un miembro con `manage` o la aplicación pueden poner `sharing=false`. El snapshot se borra inmediatamente y se notifica a los suscriptores.
- Aceptación: SCN-003 pasa; el snapshot no existe tras la respuesta 200 (NFR-PRIV-002).
- Ref: `05` §7.

#### FR-SHR-004 · MUST · F3 — Un único dispositivo publicador
- Descripción: mientras `sharing=true`, solo `publisher_device_id` puede publicar. Activar compartir desde otro dispositivo del mismo subject transfiere el rol.
- Aceptación: publicar desde otro dispositivo devuelve `NOT_PUBLISHER`.
- Ref: `05` §7.

#### FR-SHR-005 · MUST · F3 — Auditoría de compartición
- Descripción: cada cambio de `sharing` genera un evento de auditoría sin coordenadas (actor, membresía, valor, instante).
- Aceptación: prueba que verifica el registro y la ausencia de campos de posición.
- Ref: `10` §8; `11` §2.

### 2.8 Ubicación (LOC)

#### FR-LOC-001 · MUST · F4 — Modelo de muestra
- Descripción: se aceptan muestras con los campos de `LocationSample` (`contracts/proto/geostream/v1/location.proto`), con obligatorios y opcionales según `09` §3.
- Aceptación: vectores `LOC-V-*` aceptados.
- Ref: `09` §3.

#### FR-LOC-002 · MUST · F4 — Validación de campos
- Descripción: rangos, finitud y coherencia de cada campo según `09` §3.
- Aceptación: todos los vectores `LOC-I-*` producen exactamente el estado, código y campo esperados.
- Ref: `09` §3; `contracts/test-vectors/location_samples.json`.

#### FR-LOC-003 · MUST · F4 — Orden y deduplicación
- Descripción: por (dispositivo, sesión) se acepta una muestra solo si su `sequence` es mayor que la última aceptada; igual → `IGNORED_DUPLICATE`; menor → `IGNORED_STALE`.
- Aceptación: vectores de secuencia y prueba de propiedad (proptest) con permutaciones y duplicados.
- Ref: `09` §4; ADR-0008.

#### FR-LOC-004 · MUST · F4 — Validación temporal
- Descripción: `captured_at` posterior a `ahora + GS_LOCATION_MAX_FUTURE_SKEW_MS` se rechaza; anterior a `ahora − GS_LOCATION_MAX_EVENT_AGE_MS` se ignora como `IGNORED_STALE`.
- Aceptación: vectores temporales en los bordes exactos de ambos límites.
- Ref: `09` §3.4.

#### FR-LOC-005 · MUST · F4 — Tiempo de recepción del servidor
- Descripción: el servidor asigna `received_at` en la aceptación; nunca acepta un valor del cliente.
- Aceptación: `LocationUpdate.received_at` ≥ instante de envío del cliente y ≠ `captured_at` salvo coincidencia.
- Ref: `08` §7.

#### FR-LOC-006 · MUST · F4 — Calidad y tipo de dato
- Descripción: `quality.kind` es obligatorio (≠ `UNSPECIFIED`). El servidor no modifica `kind`, `source` ni la posición. Los datos derivados nunca se emiten como `MEASURED`.
- Aceptación: una muestra con `kind` `UNSPECIFIED` se rechaza; la actualización distribuida conserva `kind` y `source` exactos.
- Ref: `09` §3.3.

#### FR-LOC-007 · MUST · F4 — Rate limit de publicación
- Descripción: token bucket por (conexión, sesión) con capacidad `publish_burst` y recarga cada `min_publish_interval_ms` de la política.
- Aceptación: publicar por encima del límite produce `PublishAck.request_error = RATE_LIMITED` con `retry_after_ms > 0`; el abuso sostenido cierra con 4429.
- Ref: `08` §7.4.

#### FR-LOC-008 · MUST · F4 — Sequence floor
- Descripción: `ServerHello.sequence_floor` informa la mayor secuencia aceptada conocida para el dispositivo; el SDK nunca asigna secuencias ≤ ese valor.
- Aceptación: SCN-005 pasa (reinstalación simulada con contador perdido).
- Ref: ADR-0008; `08` §6.2.

#### FR-LOC-009 · MUST · F8 — Ingesta HTTP por lotes
- Descripción: `POST /v1/ingest/locations` con token de dispositivo y scope `ingest:write`; mismas reglas de validación, orden, compartición y publicador; respuesta con resultado por muestra.
- Aceptación: los vectores de `location_samples.json` producen los mismos resultados por HTTP que por stream.
- Ref: `07` §9.8.

### 2.9 Stream (STR)

#### FR-STR-001 · MUST · F4 — Endpoint y handshake
- Descripción: `GET /v1/stream?ticket=…` con subprotocolo `gs.v1.proto` (binario) o `gs.v1.json` (texto, habilitable por `GS_STREAM_JSON_ENABLED`); primer mensaje `ClientHello` antes de `GS_STREAM_HELLO_TIMEOUT_MS`; respuesta `ServerHello`.
- Aceptación: sin subprotocolo soportado → HTTP 400; sin hello a tiempo → cierre 4002; cualquier otro mensaje antes del hello → cierre 4000.
- Ref: `08` §3–§6.

#### FR-STR-002 · MUST · F4 — Suscripción
- Descripción: `Subscribe` verifica la autorización (`read` o aplicación con `locations:read`) y responde `SubscribeAck` con información de sesión, estados de miembros y snapshot (salvo `skip_snapshot`).
- Aceptación: un no-miembro recibe `StreamError NOT_FOUND`; un miembro sin `read` recibe `PERMISSION_DENIED`; el snapshot solo contiene miembros que comparten con snapshot vigente.
- Ref: `08` §6.3.

#### FR-STR-003 · MUST · F4 — Distribución autorizada
- Descripción: las `LocationUpdate` se entregan solo a suscripciones autorizadas en el momento de la entrega (con la caché acotada de ADR-0009).
- Aceptación: SCN-004 pasa; ninguna entrega a suscripciones cuya autorización haya sido revocada más allá del plazo NFR-SEC-002.
- Ref: `08` §7.3; ADR-0009.

#### FR-STR-004 · MUST · F4 — Acks de publicación
- Descripción: cada `PublishLocation` recibe un `PublishAck` con un `SampleResult` por muestra o un `request_error` para todo el lote.
- Aceptación: el número de resultados es igual al de muestras; el orden se conserva.
- Ref: `08` §6.4.

#### FR-STR-005 · MUST · F4 — Heartbeat, inactividad y cierre
- Descripción: el cliente envía `Heartbeat` cada `heartbeat_interval_ms`; el servidor cierra con 4408 tras `idle_timeout_ms` sin recibir ninguna trama; los códigos de cierre siguen `08` §8.
- Aceptación: pruebas para cada código de cierre de la tabla.
- Ref: `08` §6.5, §8.

#### FR-STR-006 · MUST · F4 — Backpressure
- Descripción: por conexión, las ubicaciones se coalescen por (sesión, miembro) y los eventos de control van a una cola acotada `GS_STREAM_OUTBOUND_CONTROL_QUEUE`; si esta se llena, la conexión se cierra con 4005 `SLOW_CONSUMER`.
- Aceptación: un consumidor lento recibe siempre la última ubicación de cada miembro y nunca una cola ilimitada; la memoria por conexión queda acotada.
- Ref: `08` §10.

#### FR-STR-007 · MUST · F4 — Una conexión por dispositivo
- Descripción: una conexión nueva de un dispositivo desplaza a la anterior, que se cierra con 4409 `SUPERSEDED`. No aplica a principales de aplicación.
- Aceptación: SCN-008 pasa, también entre instancias en F6.
- Ref: `08` §6.7.

#### FR-STR-008 · MUST · F4 — Revocación en vivo
- Descripción: revocación, salida, pérdida de capacidad, cierre o expiración terminan las suscripciones afectadas con `SubscriptionEnded` y la razón correspondiente, y hacen que las publicaciones posteriores se rechacen.
- Aceptación: SCN-002 y SCN-006 pasan.
- Ref: `08` §9.

#### FR-STR-009 · MUST · F6 — Fan-out multi-instancia
- Descripción: con `GS_STATE_BACKEND=redis`, las actualizaciones y eventos llegan a suscriptores conectados a cualquier instancia; ninguna instancia depende de su memoria local como única fuente.
- Aceptación: los escenarios SCN-001 a SCN-008 pasan con publicador y suscriptor en instancias distintas.
- Ref: `04` §6; ADR-0010.

#### FR-STR-010 · MUST · F6 — Drenaje controlado
- Descripción: al recibir SIGTERM la instancia deja de estar *ready*, envía `ServerNotice DRAINING` con `reconnect_after_ms` aleatorio y cierra con 4503 lo que quede tras `GS_SHUTDOWN_GRACE_S`.
- Aceptación: prueba de apagado: ningún cliente pierde eventos de control al reconectarse a otra instancia.
- Ref: `08` §6.8; `16` §6.

#### FR-STR-011 · MUST · F4 — Eventos de sesión y miembros
- Descripción: los suscriptores reciben `MemberEvent` (alta, salida, revocación, cambio) y `SessionEvent` (actualización, cierre, expiración).
- Aceptación: cada transición de `05` §4–§5 produce exactamente un evento por suscriptor afectado.
- Ref: `08` §7.

### 2.10 Presencia (PRS)

#### FR-PRS-001 · MUST · F4 — Estado de miembro
- Descripción: `MemberStatus` expone `sharing`, `connectivity` del dispositivo publicador y `last_location_at`.
- Aceptación: los cambios de compartición y de conectividad generan `MemberStatus` a los suscriptores.
- Ref: `05` §8.

#### FR-PRS-002 · MUST · F4 — Gracia de desconexión
- Descripción: `OFFLINE` se emite solo si el publicador no se reconecta dentro de `GS_STREAM_OFFLINE_GRACE_MS`.
- Aceptación: una reconexión dentro de la gracia no produce eventos de conectividad.
- Ref: `05` §8.

#### FR-PRS-003 · MUST · F6 — Presencia multi-instancia
- Descripción: la presencia se mantiene en el almacén efímero con TTL y se consulta al construir `SubscribeAck`.
- Aceptación: tras la caída abrupta de una instancia, la presencia de sus dispositivos expira dentro del TTL de presencia.
- Ref: `11` §3.

#### FR-PRS-004 · MUST · F4 — Minimización de presencia
- Descripción: para miembros con `sharing=false`, `connectivity` es `UNSPECIFIED` y `last_location_at` está ausente.
- Aceptación: prueba que inspecciona `MemberStatus` de un miembro conectado que no comparte.
- Ref: `05` §8; `10` §6.

### 2.11 SDK Flutter (SDK)

#### FR-SDK-001 · MUST · F5 — Fachada pública
- Descripción: `GeoStreamClient` con API asíncrona y tipada definida en `12` §4.
- Aceptación: las firmas públicas coinciden con `12` §4; `dart analyze` sin advertencias.

#### FR-SDK-002 · MUST · F5 — Proveedor de credenciales
- Descripción: `CredentialProvider` inyectable que obtiene tokens del backend del integrador; el SDK no contiene ni acepta API keys.
- Aceptación: el SDK no expone ningún parámetro que acepte una cadena con prefijo `gsk_` (prueba estática y de tiempo de ejecución).

#### FR-SDK-003 · MUST · F5 — Permisos y servicio de ubicación
- Descripción: comprobar y solicitar permisos; distinguir denegado, denegado permanentemente, precisión reducida, servicio apagado y restricciones de background.
- Aceptación: pruebas con fuente de ubicación simulada para cada estado.
- Ref: `12` §7.

#### FR-SDK-004 · MUST · F5 — Conexión y reconexión
- Descripción: conexión perezosa; reconexión con backoff exponencial y jitter completo; comportamiento por código de cierre según `08` §8.
- Aceptación: pruebas con `fake_async` verifican tiempos de espera y la tabla de cierre.
- Ref: `12` §8.

#### FR-SDK-005 · MUST · F5 — Installation ID y secuencia
- Descripción: persistir `installation_id` y secuencia en el mismo almacén con reserva por bloques; aplicar `sequence_floor`.
- Aceptación: tras reiniciar la app, ninguna secuencia se repite; tras perder el almacén se genera un `installation_id` nuevo.
- Ref: `12` §9.

#### FR-SDK-006 · MUST · F5 — Desfase de reloj
- Descripción: estimar el desfase con `ServerHello` y `HeartbeatAck`, y expresar `captured_at` en tiempo del servidor.
- Aceptación: con un reloj simulado desfasado 10 minutos, las muestras se aceptan.
- Ref: `12` §10.

#### FR-SDK-007 · MUST · F5 — Perfiles de muestreo
- Descripción: perfiles `navigation`, `balanced`, `fleet`, `lowPower`; nunca publicar más rápido que `min_publish_interval_ms`.
- Aceptación: pruebas de temporización por perfil.
- Ref: `12` §11.

#### FR-SDK-008 · MUST · F5 — Política offline latestOnly
- Descripción: sin conexión, conservar solo la última muestra por sesión y descartarla si supera `GS_LOCATION_MAX_EVENT_AGE_MS`.
- Aceptación: tras 5 minutos offline con movimiento, al reconectar se envía como mucho una muestra por sesión.
- Ref: ADR-0014.

#### FR-SDK-009 · MUST · F5 — Estados y errores tipados
- Descripción: estados observables y jerarquía sellada de excepciones de `12` §5–§6.
- Aceptación: cada código del catálogo se mapea a una excepción concreta (prueba exhaustiva).

#### FR-SDK-010 · MUST · F5 — Android en segundo plano
- Descripción: al compartir con la app en segundo plano, usar un foreground service de tipo `location` con notificación visible.
- Aceptación: prueba manual documentada en un dispositivo Android de marca con gestión agresiva de batería y en uno con Android sin capa del fabricante.
- Ref: `12` §12.

#### FR-SDK-011 · MUST · F7 — iOS en segundo plano
- Descripción: compartir en segundo plano con `UIBackgroundModes=location`, indicador visible y permisos `WhenInUse`/`Always` según el caso.
- Aceptación: prueba manual documentada en iPhone físico con las dos versiones mayores de iOS soportadas.
- Ref: `12` §12.

#### FR-SDK-012 · MUST · F5 — Vista de sesión
- Descripción: `SessionView` combina miembros, estados y últimas ubicaciones; elimina la ubicación de un miembro cuando deja de compartir o sale.
- Aceptación: pruebas con transporte simulado.
- Ref: `12` §4.

#### FR-SDK-013 · MUST · F5 — App de ejemplo
- Descripción: app de ejemplo que crea/une sesiones, comparte y muestra miembros en lista; en F9 añade mapa.
- Aceptación: compila en CI para Android (F5) e iOS (F7).

#### FR-SDK-014 · SHOULD · F8 — Flutter Web
- Descripción: soporte web (sin segundo plano) con subprotocolo binario.
- Aceptación: la app de ejemplo funciona en Chrome contra un servidor local.

#### FR-SDK-015 · MUST · F5 — Inyección de dependencias
- Descripción: poder inyectar transporte, cliente HTTP, logger, almacén y fuente de ubicación.
- Aceptación: la suite de pruebas unitarias del SDK no usa plataforma real.

#### FR-SDK-016 · MUST · F5 — Núcleo sin mapas
- Descripción: el paquete `geostream` no depende de ningún paquete de mapas.
- Aceptación: verificación de `pubspec.yaml` en CI.

### 2.12 Mapas (MAP)

#### FR-MAP-001 · MUST · F9 — Interfaces de adaptador
- Descripción: interfaces de renderer, geocodificación y rutas en `geostream_maps` (interfaces) con implementaciones en paquetes separados.
- Aceptación: el núcleo compila y funciona sin ningún paquete de mapas.
- Ref: `13` §3.

#### FR-MAP-002 · MUST · F9 — Stacks de proveedor
- Descripción: las combinaciones se declaran como `ProviderStack`; el SDK rechaza combinaciones prohibidas por licencia (por ejemplo, geocodificación de Google con mapa no-Google).
- Aceptación: construir un stack prohibido lanza `ConfigurationException`.
- Ref: `13` §4.

#### FR-MAP-003 · MUST · F9 — Adaptador MapLibre
- Aceptación: la app de ejemplo muestra miembros sobre MapLibre con atribución correcta.
- Ref: `13` §5.

#### FR-MAP-004 · SHOULD · F9 — Adaptador Google Maps
- Aceptación: la app de ejemplo puede cambiar a Google Maps por configuración.
- Ref: `13` §5.

#### FR-MAP-005 · MUST · F9 — Interpolación visual etiquetada
- Descripción: la animación entre puntos es solo visual, desactivable y nunca se publica ni se reporta como medición.
- Aceptación: prueba que verifica que las posiciones interpoladas no salen del renderer.
- Ref: `13` §6.

#### FR-MAP-006 · MUST · F9 — Documentación de licencias
- Descripción: documentar términos, cuotas, atribución y costos de cada proveedor soportado.
- Aceptación: sección completa en `13` revisada en el PR del adaptador.

### 2.13 Webhooks (WHK)

#### FR-WHK-001 · MUST · F8 — Registro de endpoints
- Descripción: CRUD de endpoints (URL HTTPS, tipos de evento) con scope `webhooks:manage`; el secreto de firma se muestra una vez.
- Aceptación: pruebas de contrato del CRUD.
- Ref: `07` §10.

#### FR-WHK-002 · MUST · F8 — Outbox y entrega at-least-once
- Descripción: los eventos se escriben en un outbox en la misma transacción que el cambio de estado y se entregan con reintentos según `07` §10.4.
- Aceptación: matar el worker durante la entrega no pierde eventos; un receptor que falla recibe reintentos con el calendario definido.
- Ref: ADR-0015.

#### FR-WHK-003 · MUST · F8 — Firma
- Descripción: cabeceras `GS-Webhook-Id`, `GS-Webhook-Timestamp`, `GS-Webhook-Signature` (HMAC-SHA256).
- Aceptación: vector de firma de `contracts/test-vectors/webhook_signature.json` se reproduce exactamente.
- Ref: `07` §10.3.

#### FR-WHK-004 · MUST · F8 — Protección SSRF
- Descripción: rechazar URLs que resuelvan a rangos privados, loopback, link-local o metadatos de nube, salvo `GS_WEBHOOK_ALLOW_PRIVATE_NETWORKS=true`.
- Aceptación: pruebas con `127.0.0.1`, `10.0.0.0/8`, `169.254.169.254`, `::1` y nombres DNS que resuelven a ellos.
- Ref: `10` §5.

#### FR-WHK-005 · MUST · F8 — Sin coordenadas
- Descripción: ningún payload de webhook contiene coordenadas.
- Aceptación: validación de esquema y prueba que busca campos de posición en todos los tipos de evento.

### 2.14 Integración (INT)

#### FR-INT-001 · MUST · F8 — Consumidor del lado servidor
- Descripción: una aplicación con `stream:connect` y `locations:read` abre el stream y se suscribe a cualquier sesión de su tenant.
- Aceptación: escenario de flota con consumidor de aplicación pasa.
- Ref: `15` §5.

#### FR-INT-002 · MUST · F4 — Snapshot REST
- Descripción: `GET /v1/sessions/{id}/snapshot` para miembros con `read` y aplicaciones con `locations:read`.
- Aceptación: incluye `age_ms` calculado en la respuesta; vacío si la sesión tiene `snapshot_ttl_seconds=0`.
- Ref: `07` §9.6.

#### FR-INT-003 · SHOULD · F8 — CORS por aplicación
- Descripción: orígenes permitidos configurables por aplicación para clientes web.
- Aceptación: un origen no permitido no recibe cabeceras CORS.

#### FR-INT-004 · MUST · F8 — Guía de integración
- Descripción: guía con patrones, flujo de tokens del backend y verificación de webhooks, con ejemplos ejecutables.
- Aceptación: los ejemplos se ejecutan en CI contra el servidor.
- Ref: `15`.

### 2.15 Geocercas (GEO)

#### FR-GEO-001 · MUST · F11 — Gestión de geocercas
- Descripción: crear, listar, consultar y borrar geocercas (círculo, polígono) a nivel de tenant o de sesión.
- Aceptación: validación de geometría según `14` §3.

#### FR-GEO-002 · MUST · F11 — Evaluación con histéresis
- Descripción: entrada y salida evaluadas con precisión de la muestra, margen de salida y confirmación por muestras consecutivas según `14` §4.
- Aceptación: trazas sintéticas con ruido producen exactamente las transiciones esperadas.

#### FR-GEO-003 · MUST · F11 — Eventos
- Descripción: `GeofenceEvent` por stream a suscriptores de la sesión y webhooks `geofence.entered` / `geofence.exited`.
- Aceptación: SCN de geocerca pasa; los webhooks no contienen coordenadas.

#### FR-GEO-004 · SHOULD · F11 — Índice espacial en proceso
- Descripción: R-tree en memoria por tenant con límites `max_geofences_per_tenant`.
- Aceptación: benchmark con el máximo de geocercas dentro del objetivo NFR-PERF-001.

### 2.16 Privacidad y datos (PRV)

#### FR-PRV-001 · MUST · F4 — Efímero por defecto
- Descripción: sin historial; snapshot con TTL `snapshot_ttl_seconds`; borrado al dejar de compartir, al revocar y al cerrar/expirar.
- Aceptación: prueba que inspecciona PostgreSQL y el almacén efímero tras cada transición.
- Ref: ADR-0005.

#### FR-PRV-002 · MUST · F10 — Borrado y exportación de subject
- Descripción: `DELETE /v1/subjects/{id}` (borrado: revoca tokens, termina membresías, borra snapshots, anonimiza auditoría) y `GET /v1/subjects/{id}/export`.
- Aceptación: tras el borrado no queda `external_subject_id` en ninguna tabla salvo el registro anonimizado.
- Ref: `10` §7.

#### FR-PRV-003 · MUST · F10 — Jobs de retención
- Descripción: purga periódica de auditoría, claves de idempotencia, invitaciones vencidas y entregas de webhook según `GS_RETENTION_*`.
- Aceptación: pruebas con reloj controlado.

#### FR-PRV-004 · MUST · F4 — Redacción de coordenadas
- Descripción: logs, trazas, métricas y mensajes de error nunca contienen latitud/longitud, tokens, tickets, códigos de invitación ni secretos.
- Aceptación: prueba de integración captura todos los logs de la suite de escenarios y no encuentra patrones de coordenadas ni secretos.
- Ref: `10` §8.

#### FR-PRV-005 · MUST · F3 — Auditoría
- Descripción: registrar acciones sensibles (`10` §8.3) sin coordenadas.
- Aceptación: cada acción listada produce un registro.

### 2.17 Historial y trayectorias (HIS) — post-GA

#### FR-HIS-001 · COULD · F13 — Historial opt-in
- Descripción: historial por sesión solo si la política del tenant lo permite y la sesión lo activa, con retención ≤ política.
- Aceptación: requiere ADR de base legal y retención.

#### FR-HIS-002 · COULD · F13 — Cola offline acumulada
- Descripción: modo `buffered` del SDK, cifrado en reposo, solo para sesiones con historial.

#### FR-HIS-003 · COULD · F13 — Consulta de historial
- Descripción: endpoint de historial con capacidad dedicada.

#### FR-HIS-004 · COULD · F13 — Calidad de trayectoria y map-matching
- Descripción: filtros opcionales y map-matching desacoplados del canal crítico, con salida etiquetada.

### 2.18 Operación (OPS)

#### FR-OPS-001 · MUST · F0 — Binario único con roles
- Descripción: binario `geostream` con subcomandos `serve`, `migrate`, `admin`, `version`; `serve --roles api,stream,worker`.
- Aceptación: cada rol puede ejecutarse por separado y juntos.
- Ref: `04` §5.

#### FR-OPS-002 · MUST · F2 — Salud y métricas
- Descripción: `/health/live` y `/health/ready` en el puerto HTTP; `/metrics` Prometheus solo en `GS_ADMIN_ADDR`.
- Aceptación: *ready* es falso si PostgreSQL (o Redis cuando se usa) no responde.
- Ref: `16` §4.

#### FR-OPS-003 · MUST · F1 — Configuración validada
- Descripción: cargar configuración desde entorno y archivo opcional; validar rangos; fallar al arrancar con mensaje explícito.
- Aceptación: cada parámetro fuera de rango produce error de arranque con su nombre.
- Ref: `17`.

#### FR-OPS-004 · MUST · F2 — Migraciones versionadas
- Descripción: migraciones SQL secuenciales, aplicadas con `geostream migrate`; nunca destructivas sin plan de dos fases.
- Aceptación: migrar desde vacío y re-ejecutar es idempotente.
- Ref: `11` §4.

#### FR-OPS-005 · MUST · F6 — Imagen y compose
- Descripción: Dockerfile multi-etapa, imagen no-root, `deploy/docker-compose.yml` con PostgreSQL y Valkey/Redis.
- Aceptación: `docker compose up` + quickstart funcionan en CI.

#### FR-OPS-006 · MUST · F12 — Runbooks y backups
- Descripción: runbooks de `16` §8 y restauración de backup probada.
- Aceptación: ejercicio de restauración documentado con resultado.

### 2.19 Pistas posteriores a GA

#### FR-IOT-001 · COULD · F14 — Puente MQTT
- Descripción: ingesta desde un broker MQTT para dispositivos IoT. Requiere ADR.

#### FR-NAT-001 · COULD · F15 — SDKs nativos Kotlin/Swift
- Descripción: SDKs nativos; reevaluar un núcleo Rust compartido. Requiere ADR.

#### FR-E2E-001 · COULD · F16 — Modo cifrado de extremo a extremo
- Descripción: el servidor distribuye coordenadas cifradas que no puede leer. Incompatible con snapshots en claro y geocercas del servidor. Requiere ADR y revisión criptográfica externa.

#### FR-P2P-001 · COULD · F16 — Transporte P2P WebRTC
- Descripción: canal P2P con señalización, STUN/TURN y fallback al servidor. Requiere ADR.

---

## 3. Requisitos no funcionales

Los valores son **objetivos a medir** con el método indicado. No son SLA. Los perfiles de carga están en `18_TESTING_STRATEGY.md` §8.

#### NFR-PERF-001 · MUST · F6 — Latencia interna del servidor
- Descripción: desde la recepción completa de un `PublishLocation` hasta la escritura de la `LocationUpdate` en el socket del suscriptor: p99 ≤ 50 ms en perfil P-M, nodo único.
- Aceptación: benchmark reproducible con `gs-loadgen` y métrica `gs_stream_delivery_latency_seconds`.

#### NFR-PERF-002 · MUST · F6 — Latencia extremo a extremo
- Descripción: publicación → entrega en el cliente suscriptor dentro de la misma región: p95 ≤ 300 ms en perfil P-M.
- Aceptación: medido con `gs-loadgen` (clientes en la misma región que el servidor).

#### NFR-PERF-003 · MUST · F6 — Capacidad de un nodo de referencia
- Descripción: un nodo de 4 vCPU y 8 GiB sostiene el perfil P-M durante 30 minutos con CPU < 70 % y sin errores > 0,1 %.
- Aceptación: informe de benchmark en `docs/benchmarks/` con hardware, versión y resultados.

#### NFR-PERF-004 · SHOULD · F6 — Memoria por conexión
- Descripción: ≤ 64 KiB promedio por conexión inactiva suscrita a 1 sesión.
- Aceptación: medido con 10 000 conexiones.

#### NFR-SCALE-001 · MUST · F6 — Escalado horizontal
- Descripción: perfil P-L en 3 nodos de stream con Redis/Valkey sin cambiar el protocolo.
- Aceptación: benchmark P-L documentado.

#### NFR-SCALE-002 · MUST · F6 — Tormenta de reconexión
- Descripción: 10 000 clientes que se reconectan en una ventana de 60 s con backoff y jitter completan la reconexión con < 1 % de errores.
- Aceptación: prueba de carga dedicada.

#### NFR-AVAIL-001 · SHOULD · F12 — Objetivo de disponibilidad
- Descripción: objetivo de diseño 99,9 % mensual para el plano de stream tras GA; publicar SLO solo tras 30 días de medición.
- Aceptación: tablero de SLO operativo.

#### NFR-SEC-001 · MUST · F2 — Transporte cifrado
- Descripción: TLS 1.2+ en todo endpoint remoto (terminado en proxy o balanceador); HSTS en despliegues públicos.
- Aceptación: configuración de referencia en `deploy/` y verificación en el checklist de liberación.

#### NFR-SEC-002 · MUST · F4 — Plazo de revocación
- Descripción: desde la respuesta exitosa de una revocación hasta el efecto en el stream: p99 ≤ 2 s con el bus sano; peor caso ≤ `GS_AUTHZ_CACHE_TTL_MS` + 1 s aunque se pierdan mensajes del bus.
- Aceptación: SCN-002 mide el plazo; prueba con bus que descarta mensajes verifica el peor caso.

#### NFR-SEC-003 · MUST · F2 — Secretos fuera de registros
- Descripción: tokens, tickets, API keys, secretos de webhook y códigos de invitación nunca aparecen en logs, trazas, métricas ni errores; el parámetro `ticket` se redacta en logs de acceso.
- Aceptación: prueba de captura de logs (ver FR-PRV-004).

#### NFR-SEC-004 · MUST · F3 — Aislamiento de tenant
- Descripción: ninguna operación permite leer o modificar recursos de otro tenant.
- Aceptación: matriz negativa automatizada que cubre el 100 % de las operaciones de OpenAPI y de los mensajes de stream.

#### NFR-SEC-005 · MUST · F10 — Fuzzing
- Descripción: fuzzing del decodificador de protocolo, del parser JSON del stream y de los validadores, sin pánicos.
- Aceptación: objetivos `cargo-fuzz` ejecutados ≥ 10 minutos por objetivo en CI nocturno sin fallos.

#### NFR-SEC-006 · MUST · F0 — Auditoría de dependencias
- Descripción: `cargo deny` (licencias, advisories, orígenes) y auditoría de dependencias Dart en CI.
- Aceptación: CI falla ante advisories altas o críticas sin excepción documentada.

#### NFR-PRIV-001 · MUST · F4 — Sin persistencia de coordenadas
- Descripción: sin historial activo, ninguna coordenada se escribe en PostgreSQL, logs ni disco; el almacén efímero se configura sin persistencia en disco.
- Aceptación: prueba que inspecciona las tablas tras la suite de escenarios; `deploy/` configura Redis/Valkey sin RDB/AOF.

#### NFR-PRIV-002 · MUST · F4 — Borrado de snapshot inmediato
- Descripción: el snapshot de un miembro se borra antes de responder 200 a la detención de compartición, revocación, salida o cierre.
- Aceptación: lectura del almacén efímero inmediatamente después de la respuesta.

#### NFR-RES-001 · MUST · F9 — Independencia de proveedores
- Descripción: fallos de mapas, geocodificación o rutas no afectan al stream ni a la compartición.
- Aceptación: prueba del SDK con adaptador de mapa que falla.

#### NFR-RES-002 · MUST · F6 — Degradación ante caída de Redis/Valkey
- Descripción: con el almacén efímero caído, la instancia deja de estar *ready*, rechaza conexiones nuevas, envía `ServerNotice DEGRADED`, rechaza publicaciones con `TEMPORARILY_UNAVAILABLE` y, al recuperarse, vacía la caché de autorización y se resuscribe al bus.
- Aceptación: prueba de caos que detiene y reinicia Redis/Valkey.

#### NFR-RES-003 · MUST · F6 — Apagado ordenado
- Descripción: el apagado completa en ≤ `GS_SHUTDOWN_GRACE_S` + 5 s.
- Aceptación: prueba de apagado bajo carga.

#### NFR-MOB-001 · MUST · F7 — Matriz de dispositivos
- Descripción: pruebas manuales documentadas en: Android sin capa del fabricante, Android de marca con gestión agresiva de batería, y iPhone con las dos versiones mayores de iOS soportadas.
- Aceptación: informe en `docs/device-tests/` por release.

#### NFR-MOB-002 · SHOULD · F7 — Consumo de batería medido
- Descripción: medir el consumo por perfil de muestreo en cada plataforma y publicarlo. No hay objetivo contractual.
- Aceptación: informe por release.

#### NFR-COMPAT-001 · MUST · F5 — Matriz de compatibilidad
- Descripción: publicar versiones soportadas: Rust (toolchain fijada), Flutter/Dart mínimos, Android `minSdk`/`targetSdk`, iOS mínimo, PostgreSQL y Redis/Valkey.
- Aceptación: tabla en `README.md` verificada en CI.

#### NFR-COMPAT-002 · MUST · F5 — Compatibilidad de contratos
- Descripción: a partir de M2, `buf breaking` y `oasdiff` no reportan cambios incompatibles dentro de v1.
- Aceptación: checks obligatorios en CI.

#### NFR-MAINT-001 · MUST · F0 — Versionado
- Descripción: SemVer para servidor, contratos y SDK; `CHANGELOG.md`; deprecaciones anunciadas con ≥ 6 meses.
- Aceptación: plantilla de release y verificación en CI del changelog.

#### NFR-MAINT-002 · MUST · F1 — Cobertura de pruebas
- Descripción: cobertura de líneas ≥ 85 % en `gs-domain` y `gs-auth`; ≥ 70 % global en Rust; ≥ 70 % en el SDK.
- Aceptación: informe de cobertura en CI con umbral.

#### NFR-OBS-001 · MUST · F2 — Observabilidad estructurada
- Descripción: logs JSON con `request_id` / `connection_id`; métricas de `16` §4; trazas OTLP opcionales.
- Aceptación: cada métrica listada existe y se prueba.

#### NFR-OBS-002 · MUST · F2 — Cardinalidad acotada
- Descripción: prohibido usar como label: `session_id`, `device_id`, `subject_id`, `member_id`, coordenadas o IDs externos.
- Aceptación: prueba que inspecciona `/metrics` tras la suite de escenarios.

#### NFR-COST-001 · MUST · F6 — Operación sin proveedores comerciales
- Descripción: el núcleo funciona con tres contenedores (GeoStream, PostgreSQL, Redis/Valkey) sin servicios comerciales.
- Aceptación: compose de referencia.

#### NFR-DX-001 · MUST · F8 — Quickstart en menos de 30 minutos
- Descripción: un desarrollador levanta el servidor local, crea una API key, ejecuta un backend de tokens de ejemplo y comparte ubicación entre dos instancias de la app de ejemplo en < 30 minutos.
- Aceptación: quickstart verificado manualmente en cada release y automatizado hasta donde sea posible.

#### NFR-DOC-001 · MUST · F0 — Documentación completa
- Descripción: todo endpoint, mensaje, parámetro, métrica y código de error está documentado.
- Aceptación: checks de CI de `18` §10 (consistencia de catálogos y enlaces).
