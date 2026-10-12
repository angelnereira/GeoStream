# Changelog

Formato: [Keep a Changelog](https://keepachangelog.com/es-ES/1.1.0/). Versionado: SemVer por componente (`docs/19_ENGINEERING_STANDARDS.md` §8).

## [Unreleased]

- Etapas 1, 2, 3, 4 y 5 de CI en `.github/workflows/ci.yml` y suite de consistencia documental en `tools/` (validez JSON, reglas de dependencia de crates, catálogo de errores, claves de configuración, trazabilidad de requisitos, matriz de autenticación y enlaces Markdown) (NFR-DOC-001, NFR-SEC-006, T-F0-05).
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
