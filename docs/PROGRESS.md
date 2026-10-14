# Bitácora de avance

**ID:** GS-PRG-001 · La mantienen los agentes y desarrolladores en cada PR (`20_AGENT_PROTOCOL.md` §3, §4).

## Estado actual

| Campo | Valor |
|---|---|
| Fase activa | **F4 — Stream en nodo único** (M1) |
| Último hito alcanzado | Cierre de Fase F3 (plano de control REST) — 2026-10-13 |
| Paquete documental | v0.2.0 (2026-10-11) |
| Decisiones pendientes | ADR-0019 (licencia) — no bloquea la implementación; bloquea publicar paquetes. DR-20261012-02 (ubicación de las pruebas de invariantes de persistencia, `05` §11). DR-20261012-03 (resolución de tenant en `geostream admin`, `16` §3 vs `05` §3 INV-TEN-1) — implementado con la lectura de operador, pendiente de confirmación del propietario. DR-20261012-04 (`gs-observability` inalcanzable desde `geostream` según `tools/check_crate_deps`, frente a la frase de `04` §4.1 que lo permite) — bloquea cablear los logs JSON en `serve`. **DR-20261013-01** (cómo obtiene `geostream serve` su configuración: `04` §5 documenta `--config path.toml`, pero el diagrama de `04` §4.1 que codifica `tools/check_crate_deps` no permite que `geostream` dependa de `gs-config`) — bloquea montar `/v1` y `/v1/stream` en el binario |
| Disponibilidad de nombres | crates.io (`geostream`, `gs-protocol`, `geostream-sdk`, `geostream-server`) y pub.dev (`geostream`, `geostream_flutter`): todos disponibles (verificado 2026-10-12) |

## Tareas en curso

| Tarea | Agente / persona | Desde | Crates/paquetes que posee | Notas |
|---|---|---|---|---|
| T-F4-03 — `AuthzCache` con `mver`, invalidación por bus y revalidación del principal | Angel Nereira (con agente de código) | 2026-10-13 | `crates/gs-service`, `crates/gs-stream` | NFR-SEC-002; leer `04` §6.2, ADR-0009, `06` §7 |

## Bloqueos y solicitudes de decisión abiertas

| ID | Tarea | Resumen | Abierta |
|---|---|---|---|
| DR-20261012-02 | T-F2-02 | `05` §11 solo admite pruebas de invariantes en `gs-domain` o `gs-service`; las cuatro invariantes de persistencia (`INV-TEN-1`, `INV-SUB-1`, `INV-DEV-1`, `INV-DEV-2`) las hace cumplir el esquema y se prueban en `gs-store`. Pendiente de amendar `05` §11 | 2026-10-12 |
| DR-20261012-03 | T-F2-03 | `16` §3 documenta `app disable --app`, `key create/list --app` y `key revoke --key` sin `--tenant`, mientras INV-TEN-1 exige filtrar por `tenant_id`. Implementado con resolución de tenant desde la entidad (operador) y mutaciones acotadas al tenant resuelto; pendiente de confirmación | 2026-10-12 |
| DR-20261012-04 | T-F2-06 | El diagrama de `04` §4.1 (que codifica `tools/check_crate_deps`) impide que `geostream` dependa de `gs-observability`, pero la frase siguiente del mismo §4.1 dice que «puede ser usado por cualquier crate excepto `gs-domain`». Los logs JSON quedan implementados y probados en el crate, sin cablear en `serve` | 2026-10-12 |
| DR-20261013-01 | T-F4-02 | `04` §5 documenta `geostream serve --config path.toml` y el montaje de `/v1/stream` según el rol activo, pero el diagrama de `04` §4.1 (codificado en `tools/check_crate_deps`) no permite que `geostream` dependa de `gs-config`, que es donde vive `ServerConfig`. Opciones: permitir `geostream` → `gs-config` (recomendada), mover la carga a `gs-api`, o parsear entorno ad hoc en `main.rs` duplicando `17` §2. Bloquea montar el router REST y el de stream en el binario | 2026-10-13 |

## Fases

| Fase | Estado | Cierre | Evidencia |
|---|---|---|---|
| F0 | completada | 2026-10-12 | CI verde (etapas 1–5), geostream version, gs-protocol roundtrip, geostream-tools (Rust puro), deploy dev, nombres verificados |
| F1 | completada | 2026-10-12 | Vectores de dominio verdes (ids, location, invitation, stream), invariantes INV-* con 19 pruebas nombradas, gs-domain puro sin I/O, property tests con proptest, cobertura de líneas 90.76 % (>= 85 % NFR-MAINT-002) |
| F2 | completada | 2026-10-12 | Tokens emitidos y verificados con los casos negativos de `06` §4.3 (paso 4 incluido: tenant/aplicación desactivados, época de subject y dispositivo desactivado), migraciones idempotentes, `/health/*` y `/metrics` operativos en el listener de administración, verificador de captura de logs activo, TLS de referencia en `deploy/` |
| F3 | completada | 2026-10-13 | Todas las operaciones con `x-gs-phase` ≤ F3 implementadas y conformes: suite de conformidad OpenAPI con matriz de autorización REST completa y aislamiento de tenants (T-F3-11) |
| F4 (M1) | en curso | | T-F4-01 (tickets) y T-F4-02 (conexión: upgrade, subprotocolos, hello, máquina de estados, heartbeat y cierres) completadas con pruebas extremo a extremo contra listener real; resto de tareas pendientes |
| F5 (M2) | pendiente | | |
| F6 | pendiente | | |
| F7 (M3) | pendiente | | |
| F8 | pendiente | | |
| F9 (M4) | pendiente | | |
| F10 | pendiente | | |
| F11 | pendiente | | |
| F12 (M5) | pendiente | | |

## Registro

Formato de cada entrada (más reciente arriba):

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
