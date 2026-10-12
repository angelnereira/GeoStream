# Bitácora de avance

**ID:** GS-PRG-001 · La mantienen los agentes y desarrolladores en cada PR (`20_AGENT_PROTOCOL.md` §3, §4).

## Estado actual

| Campo | Valor |
|---|---|
| Fase activa | **F1 — Núcleo de dominio** (en curso) |
| Último hito alcanzado | Cierre de Fase F0 (Bootstrap del repositorio y CI) — 2026-10-12 |
| Paquete documental | v0.2.0 (2026-10-11) |
| Decisiones pendientes | ADR-0019 (licencia) — no bloquea la implementación; bloquea publicar paquetes |
| Disponibilidad de nombres | crates.io (`geostream`, `gs-protocol`, `geostream-sdk`, `geostream-server`) y pub.dev (`geostream`, `geostream_flutter`): todos disponibles (verificado 2026-10-12) |

## Tareas en curso

| Tarea | Agente / persona | Desde | Crates/paquetes que posee | Notas |
|---|---|---|---|---|
| — | | | | |

## Bloqueos y solicitudes de decisión abiertas

| ID | Tarea | Resumen | Abierta |
|---|---|---|---|
| — | | | |

## Fases

| Fase | Estado | Cierre | Evidencia |
|---|---|---|---|
| F0 | completada | 2026-10-12 | CI verde (etapas 1–5), geostream version, gs-protocol roundtrip, geostream-tools (Rust puro), deploy dev, nombres verificados |
| F1 | en curso | | |
| F2 | pendiente | | |
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
