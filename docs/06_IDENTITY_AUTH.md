# 06 — Identidad, autenticación y autorización

**ID:** GS-IAM-001 · **Versión:** 0.2.0 · **Decisiones:** ADR-0006, ADR-0007, ADR-0009

## 1. Principales

| Principal | Credencial | Quién la usa | Ámbito |
|---|---|---|---|
| **Application** | API key `gsk_…` en `Authorization: Bearer` | Backend del integrador (server-to-server) | Todo su tenant, limitado por los scopes de la key |
| **Device** | Token de dispositivo (JWT EdDSA) en `Authorization: Bearer` | App con el SDK | Un subject y un dispositivo, limitado por scopes del token y capacidades en cada sesión |
| **Ticket** | `gst_…` en la query `ticket` del upgrade WebSocket | SDK o backend al abrir el stream | Hereda el principal y scopes que lo emitieron; un solo uso |

Reglas absolutas:

- La identidad (tenant, aplicación, subject, dispositivo) **se deriva solo de la credencial verificada**. Cualquier ID de tenant, subject o dispositivo en un cuerpo o mensaje es dato, nunca identidad.
- El SDK **NO DEBE** aceptar ni contener API keys. Las apps solo reciben tokens de dispositivo.
- Ninguna credencial aparece en logs, trazas, métricas ni errores (NFR-SEC-003).

## 2. Flujo general

1. El administrador crea tenant, aplicación y API key con la CLI (`16_OPERATIONS.md` §3).
2. El usuario inicia sesión en la app del integrador con el mecanismo del integrador.
3. El SDK pide un token a su `CredentialProvider`, que llama al backend del integrador pasando el `installation_id`.
4. El backend del integrador llama `POST /v1/tokens` con su API key y devuelve el token a la app.
5. El SDK usa el token en REST y para pedir tickets de stream.
6. Antes de que expire, el SDK pide otro token al `CredentialProvider` (el integrador decide si el usuario sigue siendo válido). GeoStream **no emite refresh tokens**.

## 3. API keys

### 3.1 Formato

```
gsk_<key_id: 32 hex minúscula>.<secret: 43 caracteres base64url sin relleno>
```

- `key_id` = UUIDv7 de la key en hex (es el `id` de `key_…` sin prefijo).
- `secret` = 32 bytes de CSPRNG codificados en base64url sin `=`.
- Regex: `^gsk_[0-9a-f]{32}\.[A-Za-z0-9_-]{43}$`. Cualquier otra forma → 401 `UNAUTHENTICATED` sin consultar la base de datos.

### 3.2 Almacenamiento y verificación
- Se guarda `secret_sha256` (32 bytes). El secreto tiene 256 bits de entropía, por lo que SHA-256 es suficiente (no se requiere KDF lento).
- Verificación: buscar por `key_id` (caché por instancia ≤ `GS_AUTHZ_CACHE_TTL_MS`), comparar `SHA-256(secret)` en **tiempo constante** (`subtle`), comprobar `status=ACTIVE`, `expires_at` (si existe) y estado de aplicación y tenant.
- `last_used_at` se actualiza como mucho una vez por minuto por key (escritura diferida).

### 3.3 Ciclo de vida
`ACTIVE` → `REVOKED` (terminal). Opcionalmente `expires_at`. Revocar publica `ApiKeyRevoked{key_id}` en `bus:global`; las conexiones de stream de aplicación abiertas con esa key se cierran con 4401.

### 3.4 Scopes de aplicación

| Scope | Permite |
|---|---|
| `tokens:mint` | `POST /v1/tokens` |
| `sessions:read` | Leer sesiones, miembros, estados (sin coordenadas) |
| `sessions:write` | Crear, actualizar y cerrar sesiones |
| `members:write` | Alta, cambio, revocación de miembros; detener compartición de otros |
| `invitations:write` | Crear, listar y revocar invitaciones |
| `locations:read` | Leer coordenadas: snapshot REST y suscripción al stream |
| `stream:connect` | Emitir tickets de stream de aplicación |
| `subjects:manage` | Consultar subjects y dispositivos, revocar tokens, desactivar dispositivos, borrar/exportar subjects |
| `webhooks:manage` | CRUD de webhooks |
| `geofences:manage` | CRUD de geocercas (F11) |
| `pools:manage` (E4) | Crear y configurar sesiones `POOL`, `availability` y `attributes` de sus miembros |
| `pools:query` (E4) | `nearby`, `rings` y `density` de un pool |
| `pools:density` (E4) | Solo `density` (app del cliente final) |
| `tools:use` (E5) | ETA, asignación por lotes y ruta sobre grafo propio |
| `providers:manage` (E5) | Configuración BYOK del Provider Gateway |
| `history:read` (E6) | Trazas, viajes y exportaciones |
| `analytics:read` (E6) | Agregados, lugares, perfiles de velocidad, matrices OD y perfil de movilidad |
| `tags:manage` (E8) | Registro y baja de tags BLE |
| `lpwan:manage` (E8) | Integraciones LPWAN y asociaciones DevEUI/IMEI |

**Default al crear una key** (si no se especifican): los de la línea base **excepto `locations:read`**, y además **ninguno** de los nuevos (`pools:*`, `tools:use`, `providers:manage`, `history:read`, `analytics:read`, `tags:manage`, `lpwan:manage`). Leer coordenadas desde un backend requiere conceder `locations:read` explícitamente; lo mismo aplica a cualquier capacidad de la pista E: **se concede uno a uno, de forma explícita**.

## 4. Tokens de dispositivo

### 4.1 Emisión — `POST /v1/tokens`
Principal: aplicación con `tokens:mint`. Cuerpo (`TokenRequest` en OpenAPI):

| Campo | Obligatorio | Reglas |
|---|---|---|
| `external_subject_id` | sí | `05` §2 |
| `installation_id` | sí | UUID v4 canónico |
| `platform` | sí | `ANDROID`, `IOS`, `WEB`, `SERVER`, `OTHER` |
| `app_version` | no | ≤ 32 caracteres imprimibles |
| `sdk_version` | no | SemVer, ≤ 32 |
| `scopes` | no | ⊆ `application.allowed_device_scopes`; default: `allowed_device_scopes ∩ {sessions:create, invitations:redeem, stream:connect, sharing:write}` |
| `ttl_seconds` | no | `GS_TOKEN_MIN_TTL_S`..=`GS_TOKEN_MAX_TTL_S`; default `GS_TOKEN_DEFAULT_TTL_S` |

Efectos: upsert del subject (si estaba `ERASED`, crear uno nuevo); upsert del dispositivo por `(application_id, installation_id, subject_id)` actualizando `platform`, versiones y `last_seen_at`; aplicar INV-DEV-2.

Respuesta 200: `{ access_token, token_type: "Bearer", expires_at, subject_id, device_id, scopes }`.

Errores: 403 `INSUFFICIENT_SCOPE` si se piden scopes no permitidos; 422 `VALIDATION_FAILED` por campos; 403 `ACCOUNT_DISABLED` si la aplicación o el tenant están deshabilitados (detectado al autenticar la key).

### 4.2 Claims

Cabecera: `{ "alg": "EdDSA", "typ": "JWT", "kid": "<kid>" }`.

| Claim | Valor |
|---|---|
| `iss` | `GS_JWT_ISSUER` |
| `aud` | `"geostream"` |
| `sub` | `subject_id` (`sub_…`) |
| `tid` | `tenant_id` |
| `aid` | `application_id` |
| `did` | `device_id` |
| `scp` | array de scopes de dispositivo |
| `sep` | `token_epoch` del subject en el momento de emisión |
| `iat`, `nbf` | instante de emisión (segundos) |
| `exp` | `iat + ttl_seconds` |
| `jti` | UUIDv4 |

No se añaden otros claims. Los tokens no contienen datos personales.

### 4.3 Verificación (orden obligatorio)
1. Formato JWS compacto válido; `alg == EdDSA`; `kid` presente en el conjunto de verificación. Si no → 401 `UNAUTHENTICATED`.
2. Firma Ed25519 válida → si no, 401 `UNAUTHENTICATED`.
3. `iss` y `aud` exactos; `nbf ≤ now + leeway`; `exp > now − leeway` (`GS_JWT_LEEWAY_S`). Expirado → 401 `TOKEN_EXPIRED`.
4. Estado del principal (caché por instancia ≤ `GS_AUTHZ_CACHE_TTL_MS`, invalidada por `bus:global`, verdad en PostgreSQL):
   - tenant y aplicación `ACTIVE` → si no, 403 `ACCOUNT_DISABLED`;
   - subject `ACTIVE` y `token_epoch == sep` → si no, 401 `UNAUTHENTICATED`;
   - dispositivo `ACTIVE` → si no, 401 `UNAUTHENTICATED`.
5. Scope requerido por la operación ∈ `scp` → si no, 403 `INSUFFICIENT_SCOPE`.

### 4.4 Claves de firma y rotación
- Algoritmo: Ed25519 (EdDSA). Directorio `GS_JWT_KEYS_DIR` con pares `<kid>.key.pem` (PKCS#8 privada) y `<kid>.pub.pem` (SPKI pública). `kid`: `^[a-z0-9-]{1,64}$`.
- Firma con `GS_JWT_ACTIVE_KID`; verifica con todas las públicas del directorio.
- `geostream admin signing-key generate --kid <kid> --out <dir>` genera el par con permisos `0600` para la privada.
- Rotación: (1) añadir par nuevo a todas las instancias; (2) cambiar `GS_JWT_ACTIVE_KID`; (3) tras `GS_TOKEN_MAX_TTL_S` + margen, retirar el par antiguo. Publicar `SigningKeysChanged` en `bus:global` no es necesario si la configuración se recarga al reiniciar.
- Arranque: falla si `GS_JWT_ACTIVE_KID` no tiene clave privada válida.

## 5. Tickets de stream

- `POST /v1/stream/tickets` con aplicación (`stream:connect`) o dispositivo (`stream:connect`).
- Ticket: `gst_` + 32 bytes de CSPRNG en base64url (43 caracteres). Regex `^gst_[A-Za-z0-9_-]{43}$`.
- Se guarda en el almacén efímero como `{p}:ticket:{sha256_hex(ticket)}` → `TicketGrant { principal_kind, tenant_id, application_id, subject_id?, device_id?, api_key_id?, scopes, token_epoch?, auth_expires_at }` con TTL `GS_TICKET_TTL_S`.
  - `token_epoch?`: época del subject (`sep`) del token con el que se pidió el ticket; solo para principales de dispositivo. Viaja en el grant para que la conexión pueda detectar que los tokens de su subject fueron revocados sin conservar el token (`06` §7, ADR-0009).
- `auth_expires_at`: para dispositivo = `exp` del token usado; para aplicación = `now + GS_STREAM_APP_MAX_CONNECTION_AGE_S`.
- Canje en el upgrade con operación atómica *get-and-delete*. Ticket inexistente, expirado o ya usado → HTTP 401 sin upgrade.
- Respuesta: `{ ticket, expires_at, stream_url }`. El cliente conecta a `stream_url + "?ticket=" + ticket`.
- El parámetro `ticket` se redacta en todos los logs de acceso (`ticket=<redacted>`).

## 6. Scopes de dispositivo

| Scope | Permite |
|---|---|
| `sessions:create` | Crear sesiones |
| `invitations:redeem` | Canjear invitaciones |
| `stream:connect` | Emitir tickets de stream |
| `sharing:write` | Activar/detener su propia compartición y **modificar sus `consent_purposes`** (INV-SHR-5) |
| `ingest:write` | Ingesta HTTP por lotes (F8) y **ingesta de historial** (E6); nunca por defecto |
| `pools:density` (E4) | `GET /v1/pools/{id}/density` (marcadores aproximados y anónimos) |
| `tools:use` (E5) | ETA, asignación y ruta; solo si `application.allowed_device_scopes` lo incluye |
| `tags:manage` (E8) | Registro y baja de tags BLE propiedad del subject |
| `edge:report` (E8) | Envío de reportes Beacon cifrados |
| `edge:lookup` (E8) | Consulta y descifrado de reportes de sus propios tags |

Ningún scope nuevo de dispositivo entra en el **default** de `application.allowed_device_scopes`: se habilita explícitamente por aplicación.

Las operaciones de lectura sobre sesiones donde el subject es miembro no requieren scope adicional: las gobiernan las capacidades.

`application.allowed_device_scopes` (default: los cinco de la línea base) limita lo que una aplicación puede pedir para sus dispositivos. Ejemplo: una app de pasajeros sin `sessions:create`.

## 7. Revocación

| Acción | Endpoint / origen | Efecto REST | Efecto stream | Plazo |
|---|---|---|---|---|
| Revocar membresía | `DELETE …/members/{id}` | Inmediato (PG) | `SubscriptionEnded`, rechazo de publicaciones | NFR-SEC-002 |
| Detener compartición | `PUT …/sharing {false}` | Inmediato | `MemberStatus`, rechazo `NOT_SHARING` | NFR-SEC-002; snapshot NFR-PRIV-002 |
| Revocar tokens del subject | `POST /v1/subjects/{id}/revoke-tokens` | ≤ NFR-SEC-002 (caché invalidada por bus) | Cierre 4401 de conexiones del subject | NFR-SEC-002 |
| Desactivar dispositivo | `DELETE /v1/devices/{id}` | ≤ NFR-SEC-002 | Cierre 4401; si era publicador, `sharing=false` | NFR-SEC-002 |
| Revocar API key | CLI | ≤ NFR-SEC-002 | Cierre 4401 de conexiones de aplicación de esa key | NFR-SEC-002 |
| Deshabilitar aplicación/tenant | CLI | 403 `ACCOUNT_DISABLED` ≤ NFR-SEC-002 | Cierre 4403 | NFR-SEC-002 |
| Retirar consentimiento `HISTORY`/`ANALYTICS` | `PUT …/sharing` (propio dispositivo, `sharing:write`) | Inmediato: escrituras de historial y analítica detenidas | — | inmediato, sin coordinación entre instancias |
| Revocar credencial BYOK de proveedor | `DELETE /v1/providers/{port}` | Inmediato: el gateway deja de usar ese proveedor | — | inmediato |
| Expiración de token | — | 401 `TOKEN_EXPIRED` | `AuthExpiring`, luego 4401 si no hay `AuthRefresh` | exacto ± leeway |

Revocar tokens **no** cambia la compartición (es una acción de seguridad; el usuario puede volver a autenticarse). Desactivar un dispositivo (cierre de sesión del integrador) **sí** detiene la compartición donde sea publicador.

Toda conexión de stream revalida su principal como mucho cada `GS_AUTHZ_CACHE_TTL_MS` (al procesar mensajes o por temporizador), de modo que la pérdida de un mensaje del bus no extiende el acceso más allá de NFR-SEC-002.

## 8. Modelo de autorización

### 8.1 Orden de evaluación (REST y stream)
1. Autenticación (§3.2 / §4.3 / §5).
2. Scope de la operación.
3. Pertenencia al tenant del recurso. Si el recurso es de otro tenant o no existe → 404 `NOT_FOUND`.
4. Relación con el recurso (dispositivo): ¿miembro `ACTIVE` de la sesión? Si no → 404 `NOT_FOUND` (FR-API-006).
5. Capacidad requerida → 403 `PERMISSION_DENIED`.
6. Estado del recurso (sesión activa, sharing, publicador) → códigos de `09` §6.
7. Reglas de dominio (INV-*) → 403/409/422 según `09` §6.

### 8.2 Matriz REST

La fuente canónica de esta matriz son las extensiones `x-gs-principals`, `x-gs-scope` y `x-gs-capability` de cada operación en `contracts/openapi/geostream.v1.yaml`. Esta tabla es su versión legible; si discrepan, detener y corregir (un check de CI compara ambas).

Leyenda: ✗ = principal no permitido (403 `PERMISSION_DENIED`); — = sin requisito.

| Operación (`operationId`) | Aplicación: scope | Dispositivo: scope | Dispositivo: capacidad en sesión |
|---|---|---|---|
| `mintToken` | `tokens:mint` | ✗ | — |
| `createStreamTicket` | `stream:connect` | `stream:connect` | — |
| `createSession` | `sessions:write` | `sessions:create` | — |
| `listSessions` | `sessions:read` | — | (solo sesiones donde es miembro activo) |
| `getSession` | `sessions:read` | — | miembro activo |
| `updateSession` | `sessions:write` | — | `manage` |
| `closeSession` | `sessions:write` | — | `manage` |
| `listMembers` | `sessions:read` | — | `read` o `manage` |
| `addMember` | `members:write` | ✗ | — |
| `getMember` | `sessions:read` | — | `read` o `manage`, o `member_id=me` |
| `updateMember` | `members:write` | — | `manage` (INV-MEM-2, INV-MEM-3) |
| `revokeMember` | `members:write` | — | `manage` |
| `leaveSession` | ✗ | — | miembro activo |
| `setSharing` | `members:write` (solo `false`) | `sharing:write` | `true`: solo `me` con `publish`; `false`: `me`, o `manage` |
| `createInvitation` | `invitations:write` | — | `invite` |
| `listInvitations` | `invitations:write` | — | `manage` (todas) o `invite` (propias) |
| `revokeInvitation` | `invitations:write` | — | `manage` o emisor |
| `redeemInvitation` | ✗ | `invitations:redeem` | — |
| `getSnapshot` | `locations:read` | — | `read` |
| `findSubjects` | `subjects:manage` | ✗ | — |
| `getSubject` | `subjects:manage` | ✗ | — |
| `revokeSubjectTokens` | `subjects:manage` | ✗ | — |
| `eraseSubject` (F10) | `subjects:manage` | ✗ | — |
| `exportSubject` (F10) | `subjects:manage` | ✗ | — |
| `getDevice` | `subjects:manage` | — | (solo dispositivos de su subject) |
| `deactivateDevice` | `subjects:manage` | — | (solo el propio dispositivo) |
| `ingestLocations` (F8) | ✗ | `ingest:write` | `publish` + `sharing` + publicador |
| `createWebhook`, `listWebhooks`, `getWebhook`, `deleteWebhook`, `testWebhook` (F8) | `webhooks:manage` | ✗ | — |
| `createGeofence`, `listGeofences`, `getGeofence`, `deleteGeofence` (F11) | `geofences:manage` | — | `manage` (solo geocercas de su sesión) |
| `createPool` (E4) | `pools:manage` | ✗ | — |
| `getPool` (E4) | `sessions:read` | ✗ | — |
| `getPoolNearby` (E4) | `pools:query` | ✗ | — |
| `getPoolRings` (E4) | `pools:query` | ✗ | — |
| `getPoolDensity` (E4) | `pools:query` | `pools:density` | — |
| `setOwnAvailability` (E4) | — | `sharing:write` | `me`, solo en sesión `POOL` |
| `setPoolMemberAttributes` (E4) | `pools:manage` | ✗ | — |
| `resetOdometer` (E2) | `members:write` | ✗ | — |
| `estimateEta` (E5) | `tools:use` | `tools:use` (si está permitido) | — |
| `assignBatch` (E5) | `tools:use` | `tools:use` (si está permitido) | — |
| `computeRoute` (E7) | `tools:use` | ✗ | — |
| `getProvider`, `putProvider`, `deleteProvider` (E5) | `providers:manage` | ✗ | — |
| `ingestHistory` (E6) | ✗ | `ingest:write` | `publish` + `sharing` + publicador |
| `getTrace` (E6) | `history:read` | — | `read` y sesión `ACTIVE` |
| `listTrips`, `getTrip` (E6) | `history:read` | ✗ | — |
| `getHeatmap` (E6) | `analytics:read` | ✗ | — |
| `getOdMatrix` (E6) | `analytics:read` | ✗ | — |
| `getPlaces` (E7) | `analytics:read` | ✗ | — |
| `getSpeedProfile` (E7) | `analytics:read` | ✗ | — |
| `getMobilityProfile` (E6) | `analytics:read` | ✗ | — |
| `createExport`, `getExport` (E6) | `history:read` | ✗ | — |
| `createTag`, `listTags`, `getTag`, `deleteTag` (E8) | `tags:manage` | `tags:manage` | — |
| `postEdgeReport` (E8) | ✗ | `edge:report` | — |
| `lookupEdgeReports` (E8) | ✗ | `edge:lookup` | — |
| `ingestLpwan` (E8) | — | ✗ | — |

`ingestLpwan` no usa scopes: se autentica con la **firma HMAC de la integración** sobre el cuerpo bruto (`X-Gs-Lpwan-Signature`, `X-Gs-Lpwan-Timestamp`), con el secreto cuyo hash guarda `edge_lpwan_integrations`. La columna de aplicación es `—` (sin scope adicional), no una credencial de tenant: la integración es de red, no un backend.
| `healthLive`, `healthReady` | sin autenticación | sin autenticación | — |

### 8.3 Matriz de mensajes de stream

| Mensaje | Aplicación | Dispositivo |
|---|---|---|
| `Subscribe` | `locations:read` (cualquier sesión del tenant) | capacidad `read` |
| `Subscribe` (`replay_trace = true`, E6) | `locations:read` + `history:read` | capacidad `read` |
| `Unsubscribe` | — | — |
| `PublishLocation` | ✗ (`StreamError PERMISSION_DENIED`) | `publish` ∧ `sharing=true` ∧ es `publisher_device_id` |
| `Heartbeat` | — | — |
| `AuthRefresh` | ✗ (las conexiones de aplicación se renuevan reconectando) | token válido del **mismo** `did` |

**Minimización en `MemberStatus`:** `availability` y `attributes` de sesiones `POOL` solo se proyectan a principales de aplicación. Para una conexión de dispositivo se omiten siempre (INV-PRS-2), aunque la membresía tenga `read`.

## 9. Protección de autenticación
- Fallos de autenticación por IP: límite `GS_AUTH_FAILURE_RPM_PER_IP`; al excederse, 429 durante el resto de la ventana.
- Respuestas de error de autenticación idénticas en forma y tiempo aproximado para key inexistente y secreto incorrecto.
- CORS: solo orígenes en `application.allowed_origins` (F8); por defecto ninguno.

## 10. Pruebas obligatorias
- Matriz REST completa: cada operación × {sin credencial, key sin scope, key con scope, key de otro tenant, token sin scope, token con scope, token de no miembro, token de miembro sin capacidad, token de miembro con capacidad}.
- Matriz de stream equivalente.
- Cada paso de §4.3 con su caso negativo.
- Revocaciones de §7 con medición de plazo.
