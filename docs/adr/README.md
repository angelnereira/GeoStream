# Registro de decisiones de arquitectura (ADR)

Formato: `docs/templates/ADR_TEMPLATE.md`. Estados: Propuesto, Aceptado, Rechazado, Sustituido por ADR-NNNN.

Los ADR marcados **Aceptado (baseline v0.2.0)** forman parte del diseño inicial entregado con este paquete y se tratan como vigentes para implementar. El propietario puede sustituir cualquiera mediante un ADR nuevo. Los ADR aceptados no se editan salvo erratas; un cambio de decisión crea un ADR que sustituye al anterior.

| ADR | Título | Estado |
|---|---|---|
| [ADR-0001](ADR-0001-docs-governed-development.md) | Desarrollo gobernado por la documentación | Aceptado (baseline v0.2.0) |
| [ADR-0002](ADR-0002-modular-monolith.md) | Monolito modular en un binario con roles | Aceptado (baseline v0.2.0) |
| [ADR-0003](ADR-0003-rust-backend-stack.md) | Backend en Rust con Tokio, Axum, sqlx y Protobuf | Aceptado (baseline v0.2.0) |
| [ADR-0004](ADR-0004-transport-wss-protobuf.md) | WebSocket seguro con Protobuf binario y mapeo JSON | Aceptado (baseline v0.2.0) |
| [ADR-0005](ADR-0005-ephemeral-by-default.md) | Ubicación efímera por defecto | Sustituido por [ADR-0024](ADR-0024-opt-in-history-and-k-anonymous-analytics.md) |
| [ADR-0006](ADR-0006-identity-model.md) | Modelo de identidad: API keys de aplicación y tokens de dispositivo emitidos por GeoStream | Aceptado (baseline v0.2.0) |
| [ADR-0007](ADR-0007-stream-tickets.md) | Autenticación del stream con tickets de un solo uso y renovación dentro de la conexión | Aceptado (baseline v0.2.0) |
| [ADR-0008](ADR-0008-sequence-per-installation.md) | Secuencia monotónica por instalación con sequence floor del servidor | Aceptado (baseline v0.2.0) |
| [ADR-0009](ADR-0009-authorization-cache-revocation.md) | Caché de autorización acotada e invalidación por bus para la revocación | Aceptado (baseline v0.2.0) |
| [ADR-0010](ADR-0010-ephemeral-and-bus-backends.md) | Backends del almacén efímero y del bus: memoria y Redis/Valkey | Aceptado (baseline v0.2.0) |
| [ADR-0011](ADR-0011-postgresql-and-ids.md) | PostgreSQL con sqlx e IDs públicos prefijados sobre UUIDv7 | Aceptado (baseline v0.2.0) |
| [ADR-0012](ADR-0012-flutter-sdk-pure-dart.md) | SDK Flutter en Dart puro sobre plugins de plataforma, sin núcleo Rust en v1 | Sustituido por [ADR-0022](ADR-0022-shared-rust-client-core.md) |
| [ADR-0013](ADR-0013-maps-provider-stacks.md) | Mapas como adaptadores agrupados en stacks; MapLibre primero | Aceptado (ampliado por [ADR-0028](ADR-0028-server-provider-gateway-byok.md)) |
| [ADR-0014](ADR-0014-offline-latest-only.md) | Política offline del SDK: solo la última muestra | Sustituido por [ADR-0025](ADR-0025-encrypted-offline-buffered-queue.md) |
| [ADR-0015](ADR-0015-webhooks-outbox.md) | Webhooks con outbox transaccional, firma HMAC y entrega at-least-once | Aceptado (baseline v0.2.0) |
| [ADR-0016](ADR-0016-geofencing-in-process.md) | Geocercas evaluadas en proceso con R-tree; Tile38 y H3 diferidos | Sustituido por [ADR-0026](ADR-0026-h3-and-rtree-spatial-indexing.md) |
| [ADR-0017](ADR-0017-observability-redaction.md) | Observabilidad con tracing y Prometheus; redacción por tipos | Aceptado (baseline v0.2.0) |
| [ADR-0018](ADR-0018-errors-problem-details.md) | Errores REST con Problem Details y catálogo compartido con el stream | Aceptado (baseline v0.2.0) |
| [ADR-0019](ADR-0019-license.md) | Licencia del proyecto | Propuesto — requiere decisión del propietario |
| [ADR-0020](ADR-0020-transitive-permissive-licenses.md) | Excepciones de licencias permisivas para dependencias transitivas | Aceptado |
| [ADR-0021](ADR-0021-location-and-mobility-engine.md) | GeoStream como motor de ubicación y movilidad (alcance y no objetivos) | Aceptado |
| [ADR-0022](ADR-0022-shared-rust-client-core.md) | Núcleo Rust compartido entre servidor y dispositivo (sans-IO + runtime) | Aceptado (sustituye ADR-0012) |
| [ADR-0023](ADR-0023-server-tracking-engine.md) | Motor de trayectoria en el servidor con estimación etiquetada | Aceptado |
| [ADR-0024](ADR-0024-opt-in-history-and-k-anonymous-analytics.md) | Historial opt-in por política, modo de sesión y consentimiento con propósito; analítica con k-anonimato | Aceptado (sustituye ADR-0005) |
| [ADR-0025](ADR-0025-encrypted-offline-buffered-queue.md) | Cola offline `buffered` cifrada para sesiones con historial | Aceptado (sustituye ADR-0014) |
| [ADR-0026](ADR-0026-h3-and-rtree-spatial-indexing.md) | H3 (`h3o`) para el índice vivo y los agregados; R-tree para polígonos | Aceptado (sustituye ADR-0016) |
| [ADR-0027](ADR-0027-mobility-pools-and-live-index.md) | Pools como modo de sesión e índice vivo con dos backends | Aceptado |
| [ADR-0028](ADR-0028-server-provider-gateway-byok.md) | Provider Gateway en el servidor con BYOK y manifiestos (adopción de A-01 §5) | Aceptado (enmienda ADR-0013) |
| [ADR-0029](ADR-0029-geostream-beacon-ble-network-and-dult.md) | Red de búsqueda GeoStream Beacon: criptografía, trama y alineación con DULT | Aceptado |
| [ADR-0030](ADR-0030-client-visual-prediction.md) | Predicción visual en el cliente | Aceptado |
| [ADR-0031](ADR-0031-ffi-surface-and-language-bindings.md) | Herramientas de bindings por lenguaje y excepción de `unsafe` en `gs-ffi` | Aceptado |
| [ADR-0032](ADR-0032-learned-map-layer-and-own-road-graph.md) | Grafo vial propio opcional (OSM/ODbL) y map-matching integrado | Aceptado |
| [ADR-0033](ADR-0033-developer-console-and-gravital-oidc.md) | Consola de desarrolladores con inicio de sesión Gravital (OIDC), registro público y cuotas | Aceptado |
| [ADR-0034](ADR-0034-public-portal-landing-docs-demo-and-own-tiles.md) | Portal público (landing, documentación, estado y demo) y mapa propio | Aceptado |
| [ADR-0035](ADR-0035-same-origin-api-proxy-for-the-portal.md) | El portal llama a la API por un proxy del mismo origen, sin CORS | Aceptado |

Nota: ADR-0014 sustituyó el requisito FR-033 de la propuesta 0.1.0, y ADR-0025 reintroduce la cola `buffered` cifrada exclusivamente para sesiones con historial activo (`history_mode ≠ OFF`).
