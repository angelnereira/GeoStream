# 09 — Reglas de validación y catálogo de errores

**ID:** GS-VAL-001 · **Versión:** 0.2.0
**Contratos relacionados:** `contracts/test-vectors/location_samples.json`, `contracts/test-vectors/ids.json`, enum `ErrorCode` en `common.proto`, esquema `Problem` en OpenAPI.

## 1. Principios

1. Todo dato que llega de un cliente es no confiable y se valida en el servidor, aunque el SDK también lo valide.
2. La validación es **pura y determinista** (`gs-domain`): dado el mismo input, el mismo `now` y la misma política, el mismo resultado.
3. Se falla de forma cerrada: ante duda, rechazar.
4. Un error informa del **primer** fallo según el orden definido (orden estable = pruebas estables).
5. Los mensajes de error no contienen el valor recibido cuando puede ser sensible (coordenadas, códigos, tokens). Indican el campo y la razón.

## 2. Reglas generales de REST

| Regla | Detalle | Error |
|---|---|---|
| Tamaño de cuerpo | ≤ `GS_REST_MAX_BODY_BYTES` | 413 `PAYLOAD_TOO_LARGE` |
| Content-Type | `application/json` en peticiones con cuerpo | 415 `VALIDATION_FAILED` |
| JSON malformado | | 400 `VALIDATION_FAILED` |
| Campos desconocidos | Se rechazan en cuerpos de petición | 422 `VALIDATION_FAILED` (`reason: unknown_field`) |
| Ruta inexistente | | 404 `NOT_FOUND` |
| Método no permitido | | 405 `VALIDATION_FAILED` |
| ID malformado en la ruta | Se trata como inexistente | 404 `NOT_FOUND` |
| ID malformado en el cuerpo | | 422 `VALIDATION_FAILED` (`reason: invalid_format`) |
| `Idempotency-Key` | 1..=255 caracteres `[A-Za-z0-9_\-:.]` | 422 `VALIDATION_FAILED` |
| `If-Match` | `"v<entero>"` | 422 `VALIDATION_FAILED` si malformado; 412 `PRECONDITION_FAILED` si no coincide |
| Paginación | `limit` 1..=100 (default 50); `cursor` opaco ≤ 512 caracteres emitido por el servidor | 422 `VALIDATION_FAILED` |
| Cadenas | UTF-8 válido; sin caracteres de control (U+0000–U+001F, U+007F) salvo que se indique | 422 |

### 2.1 Vocabulario de `reason` en errores de campo

`required`, `invalid_format`, `out_of_range`, `not_finite`, `too_long`, `too_short`, `too_many`, `not_allowed`, `unknown_field`, `invalid_enum`, `conflict`.

Cada error de campo REST: `{ "field": "<ruta JSON con puntos>", "reason": "<reason>" }`.

## 3. Muestra de ubicación (`LocationSample`)

### 3.1 Orden de evaluación y reglas

Las reglas se evalúan **en este orden**; el primer fallo produce `SAMPLE_STATUS_REJECTED`, `error_code = INVALID_LOCATION` y `field` = la ruta indicada (en stream; en la ingesta HTTP el mismo par `field`/`error_code` va en `SampleResult`).

| # | Campo (`field`) | Presencia | Regla |
|---|---|---|---|
| 1 | `sequence` | obligatorio | entero en [1, 9007199254740991] (2^53 − 1) |
| 2 | `captured_at` | obligatorio | `Timestamp` válido: `seconds` en [−62135596800, 253402300799], `nanos` en [0, 999999999] |
| 3 | `position` | obligatorio | presente |
| 4 | `position.latitude` | obligatorio | finito, [−90, 90] |
| 5 | `position.longitude` | obligatorio | finito, [−180, 180] |
| 6 | `position` | — | si `reject_null_island` (política): rechazar si `latitude == 0.0 ∧ longitude == 0.0` exactamente |
| 7 | `position.horizontal_accuracy_m` | obligatorio | finito, > 0, ≤ `max_horizontal_accuracy_m` (política) |
| 8 | `position.altitude_m` | opcional | finito, [−1000, 50000] |
| 9 | `position.vertical_accuracy_m` | opcional | finito, (0, 10000] |
| 10 | `position.speed_mps` | opcional | finito, [0, 300] |
| 11 | `position.speed_accuracy_mps` | opcional | finito, (0, 300] |
| 12 | `position.heading_deg` | opcional | finito, [0, 360) |
| 13 | `position.heading_accuracy_deg` | opcional | finito, (0, 180] |
| 14 | `quality` | obligatorio | presente |
| 15 | `quality.kind` | obligatorio | valor conocido ≠ `UNSPECIFIED` |
| 16 | `captured_at` | — | `captured_at ≤ now + GS_LOCATION_MAX_FUTURE_SKEW_MS` |

Normalizaciones (sin error):
- `quality.source` o `quality.motion` con un valor numérico desconocido se tratan como `UNSPECIFIED`.
- En proto3, la "presencia" de `latitude`, `longitude` y `horizontal_accuracy_m` no es distinguible del valor 0; por eso `horizontal_accuracy_m = 0` falla la regla 7 y `(0, 0)` se cubre con la regla 6.

Reglas del SDK antes de enviar (`12` §11): convertir velocidades/rumbos negativos de la plataforma en "ausente", normalizar `heading == 360` a 0, nunca inventar precisión.

### 3.2 Resultados no erróneos
- `captured_at < now − GS_LOCATION_MAX_EVENT_AGE_MS` → `SAMPLE_STATUS_IGNORED_STALE` (no avanza la secuencia).
- `sequence == última aceptada` → `SAMPLE_STATUS_IGNORED_DUPLICATE`.
- `sequence < última aceptada` → `SAMPLE_STATUS_IGNORED_STALE`.

### 3.3 Calidad
- El servidor no modifica `quality`.
- Si en el futuro el servidor produce datos derivados (F13), los emitirá con `kind` ≠ `MEASURED` y nunca sobrescribirá la medición original.

### 3.4 Tiempo
- `now` = reloj del servidor en el momento de procesar el **lote** (un único valor por lote, también usado como `received_at` de sus muestras aceptadas).
- Bordes: `captured_at == now + skew` → válido; `captured_at == now − max_age` → válido (no stale).

## 4. Algoritmo de aceptación (normativo)

```text
// ILUSTRATIVO en sintaxis; NORMATIVO en orden y semántica.
fn process_publish(conn, msg: PublishLocation) -> PublishAck:
    // 1. Precondiciones del lote (08 §6.4), en el orden de la tabla.
    if let Err(code) = check_batch_preconditions(conn, msg): 
        return PublishAck { session_id, request_error: code, results: [], retry_after_ms }
    now = server_clock.now()
    policy = effective_policy(conn.tenant, session)
    results = []
    last_accepted = None
    max_seq = 0
    for s in msg.samples (en orden):
        // 2. Validación pura (§3.1, reglas 1–16)
        if let Err(field) = validate_sample(s, policy, now):
            results.push(REJECTED(s.sequence, INVALID_LOCATION, field)); continue
        // 3. Antigüedad (§3.2)
        if s.captured_at < now - max_event_age:
            results.push(IGNORED_STALE(s.sequence)); continue
        // 4. Orden (CAS atómico en el almacén efímero, clave seq:{session}:{device})
        match ephemeral.seq_compare_and_set(session, device, s.sequence):
            Duplicate => results.push(IGNORED_DUPLICATE(s.sequence))
            Stale     => results.push(IGNORED_STALE(s.sequence))
            Accepted  =>
                results.push(ACCEPTED(s.sequence))
                last_accepted = Some(s); max_seq = s.sequence
                bus.publish(loc_topic(session), LocationUpdate{session, member, s, received_at: now, LIVE})
    // 5. Efectos finales
    if let Some(s) = last_accepted:
        if session.snapshot_ttl_seconds > 0: ephemeral.set_snapshot(session, member, update(s), ttl)
        ephemeral.devseq_max(device, max_seq)
        geofence_engine.evaluate(...)   // F11, asíncrono, nunca bloquea
    return PublishAck { session_id, request_error: UNSPECIFIED, results, retry_after_ms: 0 }
```

`seq_compare_and_set` DEBE ser atómico (script Lua en Redis/Valkey; mutex en memoria). Semántica: si no existe valor previo → `Accepted`; si `seq > prev` → guardar y `Accepted`; si `seq == prev` → `Duplicate`; si `seq < prev` → `Stale`.

## 5. Validación de otros recursos

Los campos y tipos exactos están en OpenAPI. Reglas semánticas adicionales:

| Recurso / campo | Regla | Error |
|---|---|---|
| `external_subject_id` | `^[A-Za-z0-9._:\-]{1,128}$` | 422 `invalid_format` |
| `installation_id` | UUID v4 canónico en minúsculas | 422 `invalid_format` |
| `CreateSession.mode` | obligatorio, enum | 422 `required` / `invalid_enum` |
| `CreateSession.expires_in_seconds` | 60..=`max_session_ttl_seconds`; default `default_session_ttl_seconds` | 422 `out_of_range` |
| `CreateSession.max_members` | `ONE_TO_ONE`: ausente o 2; otros: 2..=`max_members_cap`; default por modo | 422 |
| `CreateSession.snapshot_ttl_seconds` | 0..=`snapshot_ttl_max_seconds`; default `snapshot_ttl_default_seconds` | 422 |
| `CreateSession.stale_after_seconds` | 5..=3600; default `stale_after_seconds_default` | 422 |
| `metadata` | ≤ 32 entradas; clave `^[a-z0-9_.-]{1,64}$`; valor ≤ 512 caracteres | 422 `too_many` / `invalid_format` / `too_long` |
| `CreateSession.owner_external_subject_id` | solo aplicación; si lo envía un dispositivo → `not_allowed` | 422 |
| `CreateSession.initial_members` | solo aplicación; ≤ min(`max_members`, 100); sin duplicados; total con propietario ≤ `max_members` | 422 |
| Sesiones activas por subject | ≤ `max_active_sessions_per_subject` al crear o unirse | 409 `CONFLICT` |
| `UpdateSession.expires_at` | > `now + 60 s` y ≤ `now + max_session_ttl_seconds` | 422 |
| `UpdateSession.max_members` | ≥ miembros activos (si no, 409 `CONFLICT`); inmutable en `ONE_TO_ONE` | 422 / 409 |
| `AddMember.role = OWNER` | solo si la sesión no tiene propietario activo | 409 `CONFLICT` |
| `UpdateMember.role` | solo aplicación | 403 `PERMISSION_DENIED` |
| `UpdateMember.capabilities` | INV-MEM-1..3 | 403 `PERMISSION_DENIED` |
| `SetSharing.sharing = true` | solo `member_id = me` con `publish` | 403 `PERMISSION_DENIED` |
| `CreateInvitation.grants` | ⊆ capacidades del emisor; `manage` solo propietario o aplicación | 403 `PERMISSION_DENIED` |
| `CreateInvitation.max_uses` | 1..=`invitation_max_uses`; default 1 | 422 |
| `CreateInvitation.expires_in_seconds` | 60..=`invitation_max_ttl_seconds`, y sin superar `session.expires_at`; default 86400 (recortado a la expiración de la sesión) | 422 |
| `RedeemInvitation.code` | 1..=32 caracteres antes de normalizar; normalización de `05` §6 | 404 `INVITATION_INVALID` si no normaliza |
| `TokenRequest.ttl_seconds` | `GS_TOKEN_MIN_TTL_S`..=`GS_TOKEN_MAX_TTL_S` | 422 |
| `TokenRequest.scopes` | ⊆ `allowed_device_scopes` | 403 `INSUFFICIENT_SCOPE` |
| `Webhook.url` | `https://` (o `http://` solo con `GS_ENV=dev`), ≤ 2048, host resoluble y no privado (FR-WHK-004) | 422 `invalid_format` / `not_allowed` |
| `Webhook.event_types` | no vacío, subconjunto del catálogo de `07` §10.2 | 422 |
| Geocercas | `14_GEOFENCING.md` §3 | 422 |
| `IngestLocations.samples` | 1..=`max_samples_per_publish_http` | 413 `PAYLOAD_TOO_LARGE` si excede |

## 6. Catálogo de errores

Fuente del nombre: `ErrorCode` en `common.proto`. En REST, `code` = nombre sin `ERROR_CODE_`.

| `code` | HTTP | Stream | Reintentable | Significado | Acción del cliente |
|---|---|---|---|---|---|
| `UNAUTHENTICATED` | 401 | `StreamError`, cierre 4401 | Tras obtener credencial nueva | Credencial ausente, inválida o revocada | Pedir token nuevo; si falla, re-login del integrador |
| `TOKEN_EXPIRED` | 401 | `StreamError` (AuthRefresh) | Sí, con token nuevo | Token expirado | `forceRefresh` |
| `PERMISSION_DENIED` | 403 | `StreamError`, `request_error` | No | Principal o capacidad insuficiente | Mostrar error; no reintentar |
| `INSUFFICIENT_SCOPE` | 403 | `StreamError` | No | Falta scope en la credencial | Corregir configuración del integrador |
| `ACCOUNT_DISABLED` | 403 | cierre 4403 | No | Tenant, aplicación (o subject) deshabilitado | Detener |
| `NOT_FOUND` | 404 | `StreamError`, `request_error` | No | Inexistente o sin relación | Tratar como inexistente |
| `CONFLICT` | 409 | — | No (salvo idempotencia en curso: sí, tras breve espera) | Estado incompatible (duplicado, límite de sesiones activas, petición idempotente en curso) | Releer estado |
| `VALIDATION_FAILED` | 400/405/415/422 | `StreamError`, `request_error` | No | Entrada inválida | Corregir petición |
| `INVALID_LOCATION` | 422 (por muestra en ingesta) | `SampleResult` | No | Muestra inválida | Descartar muestra; registrar defecto |
| `RATE_LIMITED` | 429 + `Retry-After` | `request_error` + `retry_after_ms`; cierre 4429 | Sí, tras la espera | Límite excedido | Esperar |
| `PAYLOAD_TOO_LARGE` | 413 | `request_error`; cierre 1009 | No | Cuerpo/lote/trama demasiado grande | Reducir |
| `SESSION_CLOSED` | 409 | `StreamError`, `request_error` | No | Sesión cerrada | Retirar sesión de la UI |
| `SESSION_EXPIRED` | 409 | `StreamError`, `request_error` | No | Sesión expirada | Retirar sesión |
| `SESSION_FULL` | 409 | — | No | Límite de miembros | Informar |
| `MEMBER_REVOKED` | 403 | `request_error` | No | El subject fue revocado de la sesión | Retirar sesión |
| `NOT_SHARING` | 409 | `request_error` | No | Publicación sin compartición activa | Detener publicación local |
| `NOT_PUBLISHER` | 409 | `request_error` | No | Otro dispositivo del subject es el publicador | Detener publicación local; informar |
| `INVITATION_INVALID` | 404 | — | No | Invitación no canjeable | Informar genéricamente |
| `IDEMPOTENCY_KEY_REUSED` | 422 | — | No | Misma key con cuerpo distinto | Generar key nueva |
| `PRECONDITION_FAILED` | 412 | — | Tras releer | `If-Match` no coincide | Releer y reintentar |
| `PROTOCOL_VERSION_UNSUPPORTED` | 400 (upgrade) | cierre 4001 | No | Versión de protocolo no soportada | Actualizar SDK |
| `PROTOCOL_ERROR` | — | `StreamError`; cierre 4000 | Reconectar | Mensaje malformado | Registrar defecto |
| `TOO_MANY_SUBSCRIPTIONS` | — | `StreamError` | No | Excede `max_subscriptions` | Reducir suscripciones |
| `TEMPORARILY_UNAVAILABLE` | 503 + `Retry-After` | `StreamError`, `request_error`, cierre 4503 | Sí | Dependencia caída, drenaje, sobrecarga | Reintentar con backoff |
| `INTERNAL` | 500 | `StreamError`, cierre 4500 | Sí (con backoff, máx. 3) | Error inesperado | Reintentar; reportar `request_id` |

Reglas:
- El servidor NO DEBE emitir un `code` fuera de este catálogo.
- Añadir un código es un cambio menor del contrato (requiere actualizar `common.proto`, OpenAPI, esta tabla y el mapeo del SDK en el mismo PR).
- `INTERNAL` nunca incluye detalles internos; el `request_id` permite correlacionar en logs.

## 7. Formato de error

- REST: Problem Details, ver `07_REST_API.md` §5.
- Stream: `StreamError { code, message, retry_after_ms, field, session_id }` o `PublishAck.request_error` / `SampleResult.error_code`.
- `message`: inglés, ≤ 200 caracteres, sin valores sensibles. Ejemplo: `"latitude out of range"`, nunca `"latitude 91.2 out of range"`.
