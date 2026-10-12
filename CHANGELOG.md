# Changelog

Formato: [Keep a Changelog](https://keepachangelog.com/es-ES/1.1.0/). Versionado: SemVer por componente (`docs/19_ENGINEERING_STANDARDS.md` §8).

## [Unreleased]

### Added
- `gs-domain`: generación uniforme de códigos de invitación en Crockford Base32 (16 caracteres, 80 bits de entropía), normalización estricta (exclusión de 'U', mapeo de 'I'/'L'->'1' y 'O'->'0', eliminación de guiones y espacios), hash criptográfico SHA-256 en hex minúscula y suite de pruebas parametrizada contra `contracts/test-vectors/invitation_codes.json` (FR-INV-001 base, T-F1-05).
- `gs-domain`: entidades y máquinas de estado para `Session` y `Membership`, reglas de capacidades (`Capabilities`, no reducción de propietario INV-MEM-1, anti-escalada INV-MEM-2, auto-modificación prohibida INV-MEM-3, revocación de publish detiene compartición INV-MEM-4), máquina de estados de compartición (`SharingTransition`, apagado por defecto FR-SHR-001, solo el propio subject activa INV-SHR-2, idempotencia INV-SHR-3, transferencia de publicador INV-SHR-4), proyección segura de presencia (`MemberStatusView`, INV-PRS-1), salida de propietario (FR-SES-007) y reglas de readmisión (FR-MEM-007) con 19 pruebas de invariantes (T-F1-04).
- `gs-domain`: modelo de ordenación de secuencias con almacén simulado `InMemorySequenceStore`, semántica atómica `CasResult` (Accepted, Duplicate, Stale), validación de precondiciones de lote `BatchPreconditionError` (secuencias no crecientes y límite de tamaño), pipeline normativo `process_sample_batch` y suite de pruebas parametrizada contra los 9 lotes `LOC-B-*` de `contracts/test-vectors/location_samples.json` (FR-LOC-003, T-F1-03).
- `gs-domain`: tipos de ubicación redactados (`Position` con coordenadas privadas y `Debug`/`Display` redactados `Position { <redacted> }`), enums de origen, naturaleza y movimiento, pipeline de validación puro determinista `validate_sample` (16 reglas en orden normativo de `docs/09_VALIDATION_ERRORS.md` §3.1), evaluación de ciclo de vida `evaluate_sample` y suite de pruebas de integración parametrizada contra `contracts/test-vectors/location_samples.json` (49 casos `LOC-V-*` y `LOC-I-*`, FR-LOC-001, FR-LOC-002, FR-LOC-004, FR-LOC-006, T-F1-02).
- `gs-domain`: identificadores de entidad tipados con prefijo sobre UUIDv7 (14 tipos de entidad), `ExternalSubjectId` e `InstallationId` (UUIDv4 canónico) con codec, validación estricta y suite de pruebas parametrizada contra `contracts/test-vectors/ids.json` (FR-API-006 base, T-F1-01).
- Verificación de disponibilidad de nombres de paquetes en crates.io y pub.dev, registro de recordatorio de ADR-0019 (licencia) y cumplimiento de criterios de salida para el cierre de Fase F0 (T-F0-08).
- Entorno de desarrollo local en `deploy/docker-compose.dev.yml` (PostgreSQL 16 y Valkey 7.2) y plantilla de variables de entorno de referencia `deploy/.env.example` (T-F0-07).
- Crate de herramientas del repositorio `geostream-tools` en `tools/` implementado 100 % en Rust (reemplazando scripts Python), con comandos para validar JSON de contratos, reglas de dependencias entre crates (`check_crate_deps`), catálogo de errores, claves de configuración, trazabilidad de requisitos, matriz de autenticación y enlaces Markdown (NFR-DOC-001, NFR-SEC-006, T-F0-05).
- Plantilla de release `docs/templates/RELEASE_TEMPLATE.md` y validación de formato y cambios de changelog en `geostream-tools changelog` (NFR-MAINT-001, T-F0-06).
- Workspace Cargo con la estructura de `docs/04_ARCHITECTURE.md` §4: 14 crates y 3 apps vacíos con `#![forbid(unsafe_code)]`, dependencias internas según §4.1, toolchain fijada en 1.99.0 y edición 2024 (T-F0-01).
- `gs-protocol`: compilación de contratos Protobuf en `build.rs` con `prost-build`, `pbjson-build` y `protoc-bin-vendored`, tipos `v1` con serialización JSON canónica y prueba de ida y vuelta completa JSON → binario → JSON sobre los 24 mensajes de `contracts/test-vectors/stream_messages.json` (T-F0-04).
- Binario `geostream` con subcomandos `version` (versión y commit), `serve` con listener Axum y endpoint `/health/live`, y stubs para `migrate` y `admin` (FR-OPS-001, T-F0-03).
- Dependencias Rust de la lista aprobada fijadas en `Cargo.lock`, `deny.toml` con la política de licencias de `docs/19_ENGINEERING_STANDARDS.md` §6.1, excepciones de licencias permisivas transitivas (ADR-0020) y workflow de CI con `cargo deny` y `cargo audit` (T-F0-02).

### Docs
- Propuestas no normativas en `docs/proposals/`: brechas y plan de completitud (G-01..G-22) y addendum A-01 de integración de proveedores externos. No modifican requisitos, contratos ni ADR hasta que se adopten.

## [docs-0.2.0] — 2026-10-11

### Docs
- Paquete documental completo para iniciar la implementación desde un repositorio vacío: 22 documentos normativos, 19 ADR, contratos Protobuf y OpenAPI 3.1 verificados con `buf lint` y Redocly, vectores de prueba (58 casos de ubicación, IDs, códigos de invitación, firma de webhooks, claims y 24 mensajes de stream verificados con `buf convert`), 10 escenarios ejecutables, plantillas y hoja de ruta F0–F16.

### Changed (respecto a la propuesta fundacional 0.1.0)
- Modelo de identidad definido: API keys de aplicación, tokens de dispositivo EdDSA emitidos por GeoStream, tickets de stream de un solo uso, scopes de aplicación y de dispositivo (ADR-0006, ADR-0007).
- Nueva entidad `Subject` por encima de `Device`; un solo dispositivo publicador por membresía; consentimiento del subject.
- Separación de `PublishLocation` (entrante) y `LocationUpdate` (saliente); `LocationUpdate` incluye `member_id` y `received_at`.
- Secuencia por instalación con `sequence_floor` del servidor (ADR-0008).
- Revocación con caché de autorización acotada, versión de membresías y bus (ADR-0009); plazo medible (NFR-SEC-002).
- Presencia separada en `sharing`, `connectivity` (solo si comparte) y `last_location_at`.
- Requisitos con fase y criterio de aceptación verificable.
- Geocercas con R-tree en proceso; Tile38 y H3 diferidos (ADR-0016).
- Primer adaptador de mapas: MapLibre; reglas de compatibilidad de proveedores por licencia (ADR-0013).

### Removed
- Modos de sesión `broadcast` (equivale a `ONE_TO_MANY` con límite alto) e `hybrid` (P2P pasa a pista F16).
- Cola offline acumulada obligatoria (FR-033 de 0.1.0), sustituida por `latestOnly` (ADR-0014).
