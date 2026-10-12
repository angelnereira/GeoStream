# Registro de decisiones de arquitectura (ADR)

Formato: `docs/templates/ADR_TEMPLATE.md`. Estados: Propuesto, Aceptado, Rechazado, Sustituido por ADR-NNNN.

Los ADR marcados **Aceptado (baseline v0.2.0)** forman parte del diseño inicial entregado con este paquete y se tratan como vigentes para implementar. El propietario puede sustituir cualquiera mediante un ADR nuevo. Los ADR aceptados no se editan salvo erratas; un cambio de decisión crea un ADR que sustituye al anterior.

| ADR | Título | Estado |
|---|---|---|
| [ADR-0001](ADR-0001-docs-governed-development.md) | Desarrollo gobernado por la documentación | Aceptado (baseline v0.2.0) |
| [ADR-0002](ADR-0002-modular-monolith.md) | Monolito modular en un binario con roles | Aceptado (baseline v0.2.0) |
| [ADR-0003](ADR-0003-rust-backend-stack.md) | Backend en Rust con Tokio, Axum, sqlx y Protobuf | Aceptado (baseline v0.2.0) |
| [ADR-0004](ADR-0004-transport-wss-protobuf.md) | WebSocket seguro con Protobuf binario y mapeo JSON | Aceptado (baseline v0.2.0) |
| [ADR-0005](ADR-0005-ephemeral-by-default.md) | Ubicación efímera por defecto | Aceptado (baseline v0.2.0) |
| [ADR-0006](ADR-0006-identity-model.md) | Modelo de identidad: API keys de aplicación y tokens de dispositivo emitidos por GeoStream | Aceptado (baseline v0.2.0) |
| [ADR-0007](ADR-0007-stream-tickets.md) | Autenticación del stream con tickets de un solo uso y renovación dentro de la conexión | Aceptado (baseline v0.2.0) |
| [ADR-0008](ADR-0008-sequence-per-installation.md) | Secuencia monotónica por instalación con sequence floor del servidor | Aceptado (baseline v0.2.0) |
| [ADR-0009](ADR-0009-authorization-cache-revocation.md) | Caché de autorización acotada e invalidación por bus para la revocación | Aceptado (baseline v0.2.0) |
| [ADR-0010](ADR-0010-ephemeral-and-bus-backends.md) | Backends del almacén efímero y del bus: memoria y Redis/Valkey | Aceptado (baseline v0.2.0) |
| [ADR-0011](ADR-0011-postgresql-and-ids.md) | PostgreSQL con sqlx e IDs públicos prefijados sobre UUIDv7 | Aceptado (baseline v0.2.0) |
| [ADR-0012](ADR-0012-flutter-sdk-pure-dart.md) | SDK Flutter en Dart puro sobre plugins de plataforma, sin núcleo Rust en v1 | Aceptado (baseline v0.2.0) |
| [ADR-0013](ADR-0013-maps-provider-stacks.md) | Mapas como adaptadores agrupados en stacks; MapLibre primero | Aceptado (baseline v0.2.0) |
| [ADR-0014](ADR-0014-offline-latest-only.md) | Política offline del SDK: solo la última muestra | Aceptado (baseline v0.2.0) |
| [ADR-0015](ADR-0015-webhooks-outbox.md) | Webhooks con outbox transaccional, firma HMAC y entrega at-least-once | Aceptado (baseline v0.2.0) |
| [ADR-0016](ADR-0016-geofencing-in-process.md) | Geocercas evaluadas en proceso con R-tree; Tile38 y H3 diferidos | Aceptado (baseline v0.2.0) |
| [ADR-0017](ADR-0017-observability-redaction.md) | Observabilidad con tracing y Prometheus; redacción por tipos | Aceptado (baseline v0.2.0) |
| [ADR-0018](ADR-0018-errors-problem-details.md) | Errores REST con Problem Details y catálogo compartido con el stream | Aceptado (baseline v0.2.0) |
| [ADR-0019](ADR-0019-license.md) | Licencia del proyecto | Propuesto — requiere decisión del propietario |

Nota: ADR-0014 sustituye el requisito FR-033 de la propuesta 0.1.0 (cola offline acumulada obligatoria).
