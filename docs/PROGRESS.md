# Bitácora de avance

**ID:** GS-PRG-001 · La mantienen los agentes y desarrolladores en cada PR (`20_AGENT_PROTOCOL.md` §3, §4).

## Estado actual

| Campo | Valor |
|---|---|
| Fase activa | **F1 — Núcleo de dominio** (en curso) |
| Último hito alcanzado | Cierre de Fase F0 (Bootstrap del repositorio y CI) — 2026-10-12 |
| Paquete documental | v0.2.0 (2026-10-11) |
| Decisiones pendientes | ADR-0019 (licencia) — no bloquea la implementación; bloquea publicar paquetes. DR-20261012-02 (ubicación de las pruebas de invariantes de persistencia, `05` §11). DR-20261012-03 (resolución de tenant en `geostream admin`, `16` §3 vs `05` §3 INV-TEN-1) — implementado con la lectura de operador, pendiente de confirmación del propietario. DR-20261012-04 (`gs-observability` inalcanzable desde `geostream` según `tools/check_crate_deps`, frente a la frase de `04` §4.1 que lo permite) — bloquea cablear los logs JSON en `serve` |
| Disponibilidad de nombres | crates.io (`geostream`, `gs-protocol`, `geostream-sdk`, `geostream-server`) y pub.dev (`geostream`, `geostream_flutter`): todos disponibles (verificado 2026-10-12) |

## Tareas en curso

| Tarea | Agente / persona | Desde | Crates/paquetes que posee | Notas |
|---|---|---|---|---|
| T-F2-08 — `POST /v1/tokens` con upsert de subject/dispositivo e INV-DEV-2 | Angel Nereira (con agente de código) | 2026-10-12 | `crates/gs-service`, `crates/gs-auth` | FR-AUTH-002; leer `06` §4 y OpenAPI `mintToken`; depende de T-F2-07 (cerrada) |

## Bloqueos y solicitudes de decisión abiertas

| ID | Tarea | Resumen | Abierta |
|---|---|---|---|
| DR-20261012-02 | T-F2-02 | `05` §11 solo admite pruebas de invariantes en `gs-domain` o `gs-service`; las cuatro invariantes de persistencia (`INV-TEN-1`, `INV-SUB-1`, `INV-DEV-1`, `INV-DEV-2`) las hace cumplir el esquema y se prueban en `gs-store`. Pendiente de amendar `05` §11 | 2026-10-12 |
| DR-20261012-03 | T-F2-03 | `16` §3 documenta `app disable --app`, `key create/list --app` y `key revoke --key` sin `--tenant`, mientras INV-TEN-1 exige filtrar por `tenant_id`. Implementado con resolución de tenant desde la entidad (operador) y mutaciones acotadas al tenant resuelto; pendiente de confirmación | 2026-10-12 |
| DR-20261012-04 | T-F2-06 | El diagrama de `04` §4.1 (que codifica `tools/check_crate_deps`) impide que `geostream` dependa de `gs-observability`, pero la frase siguiente del mismo §4.1 dice que «puede ser usado por cualquier crate excepto `gs-domain`». Los logs JSON quedan implementados y probados en el crate, sin cablear en `serve` | 2026-10-12 |

## Fases

| Fase | Estado | Cierre | Evidencia |
|---|---|---|---|
| F0 | completada | 2026-10-12 | CI verde (etapas 1–5), geostream version, gs-protocol roundtrip, geostream-tools (Rust puro), deploy dev, nombres verificados |
| F1 | completada | 2026-10-12 | Vectores de dominio verdes (ids, location, invitation, stream), invariantes INV-* con 19 pruebas nombradas, gs-domain puro sin I/O, property tests con proptest, cobertura de líneas 90.76 % (>= 85 % NFR-MAINT-002) |
| F2 | en curso | | |
| F3 | pendiente | | |
| F4 (M1) | pendiente | | |
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

```text
### AAAA-MM-DD — T-Fx-NN — <título>
- Autor/agente:
- Resultado: completada | parcial | bloqueada
- PR:
- Requisitos: …
- Evidencia: enlace a CI / informe
```

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
