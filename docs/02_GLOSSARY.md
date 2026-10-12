# 02 — Glosario canónico

**ID:** GS-GLO-001 · **Versión:** 0.2.0

Términos en orden alfabético. El término en `monoespaciado` es el identificador usado en código y contratos.

| Término | Identificador | Definición |
|---|---|---|
| Ack de publicación | `PublishAck` | Respuesta del servidor a un `PublishLocation`, con el resultado de cada muestra. |
| API key | `gsk_…` | Credencial server-to-server de una aplicación. Formato en `06_IDENTITY_AUTH.md` §3. |
| Aplicación | `Application` / `app_…` | Cliente registrado dentro de un tenant (por ejemplo, "app de conductores"). Posee API keys y política de scopes de dispositivo. |
| Bus | `Bus` | Canal publicación/suscripción entre instancias para eventos efímeros y de control. Implementaciones: memoria (un nodo) y Redis/Valkey Pub/Sub. |
| Capacidad | `Capabilities` | Permisos de un miembro dentro de una sesión: `publish`, `read`, `invite`, `manage`. |
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
| Captured at | `captured_at` | Instante de captura de la muestra, expresado en el dominio de tiempo del servidor (corregido por el SDK). |
