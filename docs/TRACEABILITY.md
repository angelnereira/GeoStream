# Trazabilidad de requisitos

**ID:** GS-TRC-001 · Generado desde `03_REQUIREMENTS.md` (v0.3.0).

Reglas:
- Una fila por requisito, en el orden de `03`. Nunca se borran filas (un requisito retirado se marca `RETIRADO`).
- La columna **Pruebas** se regenera con el script de `tools/` a partir de los `covers:` de las pruebas (`18` §11); hasta que exista, la mantienen los agentes a mano.
- **Estado**: `pendiente` → `en curso` → `cumplido` (código + pruebas verdes) → `verificado` (incluido en un release o hito con evidencia).
- CI falla si un requisito MUST de una fase cerrada no está `cumplido` con pruebas.

| Requisito | Prioridad | Fase | Título | Pruebas | Estado |
|---|---|---|---|---|---|
| FR-AUTH-001 | MUST | F2 | API keys de aplicación | `crates/gs-store/tests/repos_test.rs`, `crates/gs-auth/tests/api_key_test.rs` | cumplido |
| FR-AUTH-002 | MUST | F2 | Emisión de tokens de dispositivo | `crates/gs-service/tests/tokens_test.rs` | cumplido |
| FR-AUTH-003 | MUST | F2 | Verificación de tokens | `crates/gs-auth/tests/jwt_test.rs`, `crates/gs-service/tests/tokens_test.rs` | cumplido |
| FR-AUTH-004 | MUST | F2 | Rotación de claves de firma | `crates/gs-auth/tests/jwt_test.rs`, `crates/gs-auth/tests/signing_key_test.rs` | cumplido |
| FR-AUTH-005 | MUST | F3 | Revocación de tokens y desactivación de dispositivos | `crates/gs-service/tests/subjects_test.rs`, `crates/gs-api/tests/subjects_test.rs`, `crates/gs-service/tests/control_events_test.rs`, `crates/gs-stream/tests/connection_test.rs` | cumplido |
| FR-AUTH-006 | MUST | F4 | Tickets de stream de un solo uso | `crates/gs-auth/src/ticket.rs`, `crates/gs-service/tests/tickets_test.rs`, `crates/gs-api/tests/stream_ticket_test.rs` | cumplido |
| FR-AUTH-007 | MUST | F4 | Renovación de autenticación dentro de la conexión | `crates/gs-stream/src/connection.rs`, `crates/gs-stream/tests/connection_test.rs` | cumplido |
| FR-AUTH-008 | MUST | F2 | Scopes | `crates/gs-auth/tests/scopes_test.rs`, `crates/gs-api/tests/conformance_test.rs` | cumplido |
| FR-AUTH-009 | COULD | F15 | Tokens firmados por el integrador | — | pendiente |
| FR-ADM-001 | MUST | F2 | CLI de administración | `apps/geostream/tests/admin_test.rs`, `crates/gs-service/tests/admin_test.rs` | cumplido |
| FR-ADM-002 | MUST | F2 | Políticas por tenant | `crates/gs-config/tests/policy_test.rs`, `crates/gs-service/tests/admin_test.rs`, `apps/geostream/tests/admin_test.rs` | cumplido |
| FR-ADM-003 | MUST | F3 | Deshabilitar tenant o aplicación | `apps/geostream/tests/admin_test.rs`, `crates/gs-service/tests/admin_test.rs`, `crates/gs-service/tests/tokens_test.rs`, `crates/gs-api/tests/account_disabled_test.rs` | cumplido |
| FR-ADM-004 | COULD | F15 | API HTTP de administración | — | pendiente |
| FR-API-001 | MUST | F3 | Errores Problem Details | `crates/gs-domain/tests/error_mapping_test.rs`, `crates/gs-api/tests/skeleton_test.rs`, `crates/gs-api/tests/conformance_test.rs` | cumplido |
| FR-API-002 | MUST | F3 | Idempotencia | `crates/gs-service/tests/idempotency_test.rs`, `crates/gs-auth/tests/crypto_test.rs` | cumplido |
| FR-API-003 | MUST | F3 | Paginación por cursor | `crates/gs-domain/src/pagination.rs`, `crates/gs-api/src/pagination.rs` | cumplido |
| FR-API-004 | MUST | F3 | Rate limit REST | `crates/gs-api/src/rate_limit.rs`, `crates/gs-api/tests/rate_limit_test.rs` | cumplido |
| FR-API-005 | MUST | F3 | OpenAPI como fuente de verdad | `crates/gs-api/tests/conformance_test.rs` | cumplido |
| FR-API-006 | MUST | F3 | No enumeración de recursos | `crates/gs-api/tests/conformance_test.rs` | cumplido |
| FR-SES-001 | MUST | F3 | Crear sesión | `crates/gs-service/tests/sessions_test.rs`, `crates/gs-api/tests/sessions_test.rs` | cumplido |
| FR-SES-002 | MUST | F3 | Consultar y listar sesiones | `crates/gs-service/tests/sessions_test.rs`, `crates/gs-api/tests/sessions_test.rs` | cumplido |
| FR-SES-003 | MUST | F3 | Actualizar sesión | `crates/gs-service/tests/sessions_test.rs`, `crates/gs-api/tests/sessions_test.rs` | cumplido |
| FR-SES-004 | MUST | F3 | Cerrar sesión | `crates/gs-service/tests/sessions_test.rs`, `crates/gs-api/tests/sessions_test.rs` | cumplido |
| FR-SES-005 | MUST | F3 | Expiración automática | `crates/gs-service/tests/sessions_test.rs` | cumplido |
| FR-SES-006 | MUST | F3 | Modos de sesión | `tests/invariants_test.rs`, `crates/gs-service/tests/sessions_test.rs`, `crates/gs-api/tests/sessions_test.rs` | cumplido |
| FR-SES-007 | MUST | F3 | Salida del propietario | `tests/invariants_test.rs`, `crates/gs-service/tests/members_test.rs`, `crates/gs-api/tests/members_test.rs` | cumplido |
| FR-MEM-001 | MUST | F3 | Agregar miembros (aplicación) | `crates/gs-service/tests/members_test.rs`, `crates/gs-api/tests/members_test.rs` | cumplido |
| FR-MEM-002 | MUST | F3 | Listar y consultar miembros | `crates/gs-service/tests/members_test.rs`, `crates/gs-api/tests/members_test.rs` | cumplido |
| FR-MEM-003 | MUST | F3 | Cambiar capacidades | `tests/invariants_test.rs`, `crates/gs-service/tests/members_test.rs`, `crates/gs-api/tests/members_test.rs` | cumplido |
| FR-MEM-004 | MUST | F3 | Revocar miembro | `crates/gs-service/tests/members_test.rs`, `crates/gs-api/tests/members_test.rs` | cumplido |
| FR-MEM-005 | MUST | F3 | Abandonar sesión | `crates/gs-service/tests/members_test.rs`, `crates/gs-api/tests/members_test.rs` | cumplido |
| FR-MEM-006 | MUST | F3 | Límite de miembros | `crates/gs-service/tests/members_test.rs`, `crates/gs-api/tests/members_test.rs` | cumplido |
| FR-MEM-007 | MUST | F3 | Readmisión | `tests/invariants_test.rs`, `crates/gs-service/tests/members_test.rs`, `crates/gs-api/tests/members_test.rs` | cumplido |
| FR-INV-001 | MUST | F3 | Crear invitación | `tests/invitation_vectors_test.rs`, `crates/gs-service/tests/invitations_test.rs`, `crates/gs-api/tests/invitations_test.rs` | cumplido |
| FR-INV-002 | MUST | F3 | Canjear invitación | `tests/invitation_vectors_test.rs`, `crates/gs-service/tests/invitations_test.rs`, `crates/gs-api/tests/invitations_test.rs` | cumplido |
| FR-INV-003 | MUST | F3 | Listar y revocar invitaciones | `crates/gs-service/tests/invitations_test.rs`, `crates/gs-api/tests/invitations_test.rs` | cumplido |
| FR-INV-004 | COULD | F15 | Aprobación de solicitudes de unión | — | pendiente |
| FR-SHR-001 | MUST | F3 | Apagado por defecto | `crates/gs-store/tests/constraints_test.rs`, `crates/gs-service/tests/members_test.rs`, `crates/gs-api/tests/members_test.rs` | cumplido |
| FR-SHR-002 | MUST | F3 | Activación solo por el propio subject | `crates/gs-service/tests/members_test.rs`, `crates/gs-api/tests/members_test.rs` | cumplido |
| FR-SHR-003 | MUST | F3 | Detener compartición | `crates/gs-service/tests/members_test.rs`, `crates/gs-api/tests/members_test.rs` | cumplido |
| FR-SHR-004 | MUST | F3 | Un único dispositivo publicador | `crates/gs-store/tests/constraints_test.rs`, `crates/gs-service/tests/members_test.rs`, `crates/gs-api/tests/members_test.rs` | cumplido |
| FR-SHR-005 | MUST | F3 | Auditoría de compartición | `crates/gs-service/tests/audit_test.rs` | cumplido |
| FR-LOC-001 | MUST | F4 | Modelo de muestra | `tests/location_vectors_test.rs`, `crates/gs-service/tests/publishing_test.rs`, `crates/gs-stream/tests/connection_test.rs` | cumplido |
| FR-LOC-002 | MUST | F4 | Validación de campos | `tests/location_vectors_test.rs`, `crates/gs-service/tests/publishing_test.rs` | cumplido |
| FR-LOC-003 | MUST | F4 | Orden y deduplicación | `tests/location_vectors_test.rs`, `crates/gs-service/tests/publishing_test.rs`, `crates/gs-stream/tests/connection_test.rs` | cumplido |
| FR-LOC-004 | MUST | F4 | Validación temporal | `tests/location_vectors_test.rs`, `crates/gs-service/tests/publishing_test.rs`, `crates/gs-stream/tests/connection_test.rs` | cumplido |
| FR-LOC-005 | MUST | F4 | Tiempo de recepción del servidor | `crates/gs-service/tests/publishing_test.rs`, `crates/gs-stream/tests/connection_test.rs` | cumplido |
| FR-LOC-006 | MUST | F4 | Calidad y tipo de dato | `tests/location_vectors_test.rs`, `crates/gs-service/tests/publishing_test.rs` | cumplido |
| FR-LOC-007 | MUST | F4 | Rate limit de publicación | `crates/gs-service/tests/publishing_test.rs`, `crates/gs-stream/tests/connection_test.rs` | cumplido |
| FR-LOC-008 | MUST | F4 | Sequence floor | `crates/gs-service/tests/publishing_test.rs`, `crates/gs-stream/tests/connection_test.rs` | cumplido |
| FR-LOC-009 | MUST | F8 | Ingesta HTTP por lotes | — | pendiente |
| FR-STR-001 | MUST | F4 | Endpoint y handshake | `crates/gs-stream/tests/connection_test.rs`, `crates/gs-api/src/stream.rs` | cumplido |
| FR-STR-002 | MUST | F4 | Suscripción | `crates/gs-service/tests/subscriptions_test.rs`, `crates/gs-stream/tests/connection_test.rs` | cumplido |
| FR-STR-003 | MUST | F4 | Distribución autorizada | `crates/gs-stream/tests/connection_test.rs`, `crates/gs-stream/src/fanout.rs` | cumplido |
| FR-STR-004 | MUST | F4 | Acks de publicación | `crates/gs-service/tests/publishing_test.rs`, `crates/gs-stream/tests/connection_test.rs` | cumplido |
| FR-STR-005 | MUST | F4 | Heartbeat, inactividad y cierre | `crates/gs-stream/tests/connection_test.rs`, `crates/gs-stream/src/close_codes.rs` | cumplido |
| FR-STR-006 | MUST | F4 | Backpressure | `crates/gs-stream/src/fanout.rs`, `crates/gs-stream/tests/connection_test.rs` | cumplido |
| FR-STR-007 | MUST | F4 | Una conexión por dispositivo | `crates/gs-stream/src/connection.rs`, `crates/gs-stream/src/fanout.rs`, `crates/gs-stream/tests/connection_test.rs` | cumplido |
| FR-STR-008 | MUST | F4 | Revocación en vivo | `crates/gs-service/tests/control_events_test.rs`, `crates/gs-stream/tests/connection_test.rs` | cumplido |
| FR-STR-009 | MUST | F6 | Fan-out multi-instancia | `crates/gs-bus/tests/redis_bus_test.rs`, `crates/gs-ephemeral/tests/redis_ephemeral_test.rs`, `crates/gs-stream/tests/connection_test.rs` | en curso |
| FR-STR-010 | MUST | F6 | Drenaje controlado | — | pendiente |
| FR-STR-011 | MUST | F4 | Eventos de sesión y miembros | `crates/gs-service/tests/control_events_test.rs`, `crates/gs-stream/tests/connection_test.rs` | cumplido |
| FR-PRS-001 | MUST | F4 | Estado de miembro | `crates/gs-service/tests/subscriptions_test.rs`, `crates/gs-stream/src/subscription.rs` | cumplido |
| FR-PRS-002 | MUST | F4 | Gracia de desconexión | `crates/gs-stream/src/connection.rs`, `crates/gs-store/src/repos/membership_repo.rs`, `crates/gs-stream/tests/connection_test.rs` | cumplido |
| FR-PRS-003 | MUST | F6 | Presencia multi-instancia | `crates/gs-ephemeral/tests/redis_ephemeral_test.rs`, `crates/gs-stream/tests/connection_test.rs` | en curso |
| FR-PRS-004 | MUST | F4 | Minimización de presencia | `crates/gs-service/tests/subscriptions_test.rs`, `crates/gs-stream/src/subscription.rs`, `crates/gs-stream/tests/connection_test.rs` | cumplido |
| FR-SDK-001 | MUST | F5 | Fachada pública | — | pendiente |
| FR-SDK-002 | MUST | F5 | Proveedor de credenciales | — | pendiente |
| FR-SDK-003 | MUST | F5 | Permisos y servicio de ubicación | — | pendiente |
| FR-SDK-004 | MUST | F5 | Conexión y reconexión | — | pendiente |
| FR-SDK-005 | MUST | F5 | Installation ID y secuencia | — | pendiente |
| FR-SDK-006 | MUST | F5 | Desfase de reloj | — | pendiente |
| FR-SDK-007 | MUST | F5 | Perfiles de muestreo | — | pendiente |
| FR-SDK-008 | MUST | F5 | Política offline latestOnly | — | pendiente |
| FR-SDK-009 | MUST | F5 | Estados y errores tipados | — | pendiente |
| FR-SDK-010 | MUST | F5 | Android en segundo plano | — | pendiente |
| FR-SDK-011 | MUST | F7 | iOS en segundo plano | — | pendiente |
| FR-SDK-012 | MUST | F5 | Vista de sesión | — | pendiente |
| FR-SDK-013 | MUST | F5 | App de ejemplo | — (app de pruebas previa: `apps/flutter_app/test/`; no cubre el SDK) | pendiente |
| FR-SDK-014 | SHOULD | F8 | Flutter Web | — | pendiente |
| FR-SDK-015 | MUST | F5 | Inyección de dependencias | — | pendiente |
| FR-SDK-016 | MUST | F5 | Núcleo sin mapas | — | pendiente |
| FR-MAP-001 | MUST | F9 | Interfaces de adaptador | — | pendiente |
| FR-MAP-002 | MUST | F9 | Stacks de proveedor | — | pendiente |
| FR-MAP-003 | MUST | F9 | Adaptador MapLibre | — | pendiente |
| FR-MAP-004 | SHOULD | F9 | Adaptador Google Maps | — | pendiente |
| FR-MAP-005 | MUST | F9 | Predicción e interpolación visual etiquetadas | — | pendiente |
| FR-MAP-006 | MUST | F9 | Documentación de licencias | — | pendiente |
| FR-WHK-001 | MUST | F8 | Registro de endpoints | — | pendiente |
| FR-WHK-002 | MUST | F8 | Outbox y entrega at-least-once | — | pendiente |
| FR-WHK-003 | MUST | F8 | Firma | — | pendiente |
| FR-WHK-004 | MUST | F8 | Protección SSRF | — | pendiente |
| FR-WHK-005 | MUST | F8 | Sin coordenadas | — | pendiente |
| FR-INT-001 | MUST | F8 | Consumidor del lado servidor | — | pendiente |
| FR-INT-002 | MUST | F4 | Snapshot REST | `crates/gs-api/tests/sessions_test.rs::test_fr_int_002_*` | cumplido |
| FR-INT-003 | SHOULD | F8 | CORS por aplicación | — | pendiente |
| FR-INT-004 | MUST | F8 | Guía de integración | — | pendiente |
| FR-GEO-001 | MUST | F11 | Gestión de geocercas | — | pendiente |
| FR-GEO-002 | MUST | F11 | Evaluación con histéresis | — (vista previa en cliente: `apps/flutter_app/test/geofence_evaluator_test.dart`; la canónica es del servidor) | pendiente |
| FR-GEO-003 | MUST | F11 | Eventos | — | pendiente |
| FR-GEO-004 | SHOULD | F11 | Índice espacial en proceso | — | pendiente |
| FR-GEO-005 | SHOULD | E5 | Transición DWELL | — | pendiente |
| FR-GEO-006 | SHOULD | E5 | Proximidad dinámica | — | pendiente |
| FR-GEO-007 | COULD | E5 | Geocercas asistidas en el dispositivo | — | pendiente |
| FR-PRV-001 | MUST | F4 | Efímero por defecto | `crates/gs-testkit/tests/scenarios_test.rs`, `crates/gs-testkit/tests/privacy_test.rs` | cumplido |
| FR-PRV-002 | MUST | F10 | Borrado y exportación de subject | — | pendiente |
| FR-PRV-003 | MUST | F10 | Jobs de retención | — | pendiente |
| FR-PRV-004 | MUST | F4 | Redacción de coordenadas | `crates/gs-observability/tests/logging_test.rs`, `crates/gs-observability/src/log_verifier.rs`, `crates/gs-testkit/tests/privacy_test.rs` | cumplido |
| FR-PRV-005 | MUST | F3 | Auditoría | `crates/gs-store/tests/repos_test.rs`, `crates/gs-service/tests/audit_test.rs` | cumplido |
| FR-HIS-001 | MUST | E6 | Historial con política, modo y consentimiento | — | pendiente |
| FR-HIS-002 | MUST | E6 | Cola offline acumulada | — | pendiente |
| FR-HIS-003 | MUST | E6 | Consulta de trazas y viajes | — | pendiente |
| FR-HIS-004 | SHOULD | E7 | Calidad de trayectoria y map-matching | `crates/gs-mobility/tests/road_graph_map_matching_test.rs` (núcleo) | en curso |
| FR-HIS-005 | MUST | E6 | Retención por particiones y supresión del subject | — | pendiente |
| FR-HIS-006 | SHOULD | E6 | Exportación de historial | — | pendiente |
| FR-HIS-007 | COULD | E6 | Reproducción de traza en vivo (`replay_trace`) | — | pendiente |
| FR-ANA-001 | MUST | E6 | Agregados H3 con k-anonimato | — | pendiente |
| FR-ANA-002 | SHOULD | E6 | Perfil de movilidad con consentimiento | — | pendiente |
| FR-ANA-003 | SHOULD | E6 | Eventos de conducción estimados | — | pendiente |
| FR-ANA-004 | COULD | E6 | Matrices origen-destino | — | pendiente |
| FR-LRN-001 | SHOULD | E7 | Perfiles de velocidad y corrección de ETA | — | pendiente |
| FR-LRN-002 | SHOULD | E7 | Lugares aprendidos | — | pendiente |
| FR-LRN-003 | SHOULD | E7 | Map-matching integrado sobre grafo propio | `crates/gs-mobility/tests/road_graph_map_matching_test.rs` (núcleo; falta lector PBF y parámetros, DR-20261031-04) | en curso |
| FR-LRN-004 | COULD | E7 | Ruta sobre grafo propio | — | pendiente |
| FR-OPS-001 | MUST | F0 | Binario único con roles | `apps/geostream/tests/cli_test.rs` | cumplido |
| FR-OPS-002 | MUST | F2 | Salud y métricas | `crates/gs-api/tests/skeleton_test.rs`, `apps/geostream/tests/cli_test.rs` | cumplido |
| FR-OPS-003 | MUST | F1 | Configuración validada | `crates/gs-config/tests/server_config_test.rs` | cumplido |
| FR-OPS-004 | MUST | F2 | Migraciones versionadas | `migrations/0001_init.sql`, `apps/geostream/tests/cli_test.rs`, `crates/gs-store/tests/constraints_test.rs` | cumplido |
| FR-OPS-005 | MUST | F6 | Imagen y compose | — | pendiente |
| FR-OPS-006 | MUST | F12 | Runbooks y backups | — | pendiente |
| FR-OPS-007 | MUST | E1 | Compilación multiarquitectura Linux en CI | — | pendiente |
| FR-TRK-001 | MUST | E2 | Estimación por muestra | — | pendiente |
| FR-TRK-002 | MUST | E2 | Puerta de plausibilidad | — | pendiente |
| FR-TRK-003 | MUST | E2 | Anti-deriva en reposo | — | pendiente |
| FR-TRK-004 | MUST | E2 | Odómetro por (sesión, miembro) | — | pendiente |
| FR-TRK-005 | MUST | E2 | Paradas online y TrackEvent | — | pendiente |
| FR-TRK-006 | MUST | E2 | Estado con puntos de control y continuidad entre instancias | — | pendiente |
| FR-TRK-007 | SHOULD | E2 | Webhooks de paradas sin coordenadas | — | pendiente |
| FR-SMP-001 | MUST | E3 | Muestreo adaptativo por estado e intención | — | pendiente |
| FR-SMP-002 | MUST | E3 | Publicación por cambio significativo | — | pendiente |
| FR-SMP-003 | MUST | E3 | SamplingPolicy remota acotada a rangos | — | pendiente |
| FR-SMP-004 | SHOULD | E3 | Despertar por región del sistema operativo | — | pendiente |
| FR-SMP-005 | MUST | E3 | Predicción visual acotada | — | pendiente |
| FR-SMP-006 | MUST | E6 | Cola buffered cifrada y backfill de historial | — | pendiente |
| FR-CORE-001 | MUST | E3 | Núcleo sans-IO del cliente | — | pendiente |
| FR-CORE-002 | MUST | E3 | Runtime nativo del cliente | — | pendiente |
| FR-CORE-003 | MUST | E3 | Cliente REST en el núcleo | — | pendiente |
| FR-BND-001 | MUST | E3 | Binding Kotlin/Android | — | pendiente |
| FR-BND-002 | MUST | E3 | Binding Swift/iOS | — | pendiente |
| FR-BND-003 | MUST | E3 | SDK Flutter sobre el núcleo Rust | — | pendiente |
| FR-BND-004 | MUST | E9 | SDK TypeScript (Web WASM y Node) | — | pendiente |
| FR-BND-005 | SHOULD | E9 | SDK Go | — | pendiente |
| FR-BND-006 | SHOULD | E9 | SDK Ruby | — | pendiente |
| FR-BND-007 | MUST | E9 | Conformidad multilenguaje | — | pendiente |
| FR-POOL-001 | MUST | E4 | Modo POOL e invariante INV-POOL-1 | — | pendiente |
| FR-POOL-002 | MUST | E4 | Disponibilidad y atributos con minimización | — | pendiente |
| FR-POOL-003 | MUST | E4 | Índice espacial vivo con borrado síncrono | — | pendiente |
| FR-POOL-004 | MUST | E4 | Búsqueda de cercanos (`nearby`) | — | pendiente |
| FR-POOL-005 | SHOULD | E4 | Anillos de expansión (`rings`) | — | pendiente |
| FR-POOL-006 | SHOULD | E4 | Densidad aproximada y anónima (`density`) | — | pendiente |
| FR-MOB-001 | MUST | E5 | Estimación de ETA por niveles | — | pendiente |
| FR-MOB-002 | MUST | E5 | Asignación óptima por lotes | — | pendiente |
| FR-MOB-003 | MUST | E5 | Provider Gateway BYOK | — | pendiente |
| FR-EDGE-001 | MUST | E8 | Registro de tags sin EIK | — | pendiente |
| FR-EDGE-002 | MUST | E8 | Reportes Beacon cifrados E2E y lookup | — | pendiente |
| FR-EDGE-003 | MUST | E8 | Relevo en el núcleo del cliente | — | pendiente |
| FR-EDGE-004 | MUST | E8 | Protección contra rastreo no deseado (DULT) | — | pendiente |
| FR-EDGE-005 | SHOULD | E8 | Ingesta LPWAN | — | pendiente |
| FR-EDGE-006 | COULD | E8 | Pasarela de rastreadores celulares | — | pendiente |
| FR-EDGE-007 | COULD | E8 | Puente de tag a dispositivo virtual | — | pendiente |
| FR-EDGE-008 | COULD | E8 | Firmware de referencia para tag BLE | — | pendiente |
| FR-IOT-001 | COULD | F14 | Puente MQTT | — | pendiente |
| FR-NAT-001 | RETIRADO | F15 | SDKs nativos Kotlin/Swift | — | RETIRADO |
| FR-E2E-001 | COULD | F16 | Modo cifrado de extremo a extremo en sesiones | — | pendiente |
| FR-P2P-001 | COULD | F16 | Transporte P2P WebRTC | — | pendiente |
| NFR-PERF-001 | MUST | F6 | Latencia interna del servidor | — | pendiente |
| NFR-PERF-002 | MUST | F6 | Latencia extremo a extremo | — | pendiente |
| NFR-PERF-003 | MUST | F6 | Capacidad de un nodo de referencia | — | pendiente |
| NFR-PERF-004 | SHOULD | F6 | Memoria por conexión | — | pendiente |
| NFR-PERF-005 | MUST | E2 | Latencia del motor de trayectoria por muestra | — | pendiente |
| NFR-PERF-006 | MUST | E2 | Latencia interna con el motor activo | — | pendiente |
| NFR-PERF-007 | MUST | E4 | Latencia de consulta `nearby` en pool | — | pendiente |
| NFR-PERF-008 | MUST | E5 | Latencia de asignación por lotes | — | pendiente |
| NFR-PERF-009 | MUST | E6 | Pérdida acotada del escritor de historial | — | pendiente |
| NFR-QUAL-001 | MUST | E2 | Calidad de estimación del filtro de Kalman | — | pendiente |
| NFR-QUAL-002 | MUST | E2 | Precisión y estabilidad del odómetro | — | pendiente |
| NFR-QUAL-003 | MUST | E2 | Precisión y recobrado de detección de paradas | — | pendiente |
| NFR-SCALE-001 | MUST | F6 | Escalado horizontal | — | pendiente |
| NFR-SCALE-002 | MUST | F6 | Tormenta de reconexión | — | pendiente |
| NFR-AVAIL-001 | SHOULD | F12 | Objetivo de disponibilidad | — | pendiente |
| NFR-SEC-001 | MUST | F2 | Transporte cifrado | `deploy/caddy/Caddyfile`, `deploy/nginx/geostream.conf`, `docs/templates/RELEASE_TEMPLATE.md` | cumplido |
| NFR-SEC-002 | MUST | F4 | Plazo de revocación | `crates/gs-service/tests/authz_test.rs`, `crates/gs-stream/tests/connection_test.rs` | cumplido |
| NFR-SEC-003 | MUST | F2 | Secretos fuera de registros | `crates/gs-observability/tests/logging_test.rs`, `crates/gs-observability/src/redacted.rs`, `crates/gs-testkit/tests/privacy_test.rs` | cumplido |
| NFR-SEC-004 | MUST | F3 | Aislamiento de tenant | `crates/gs-api/tests/conformance_test.rs`, `crates/gs-testkit/tests/scenarios_test.rs` | cumplido |
| NFR-SEC-005 | MUST | F10 | Fuzzing | — | pendiente |
| NFR-SEC-006 | MUST | F0 | Auditoría de dependencias | CI (`cargo deny`, `cargo audit`) | cumplido |
| NFR-PRIV-001 | MUST | F4 | Sin persistencia de coordenadas | `crates/gs-testkit/tests/privacy_test.rs` | cumplido |
| NFR-PRIV-002 | MUST | F4 | Borrado de snapshot inmediato | `crates/gs-testkit/tests/privacy_test.rs` | cumplido |
| NFR-PRIV-003 | MUST | E6 | k-anonimato en todo agregado expuesto | — | pendiente |
| NFR-PRIV-004 | MUST | E8 | Reportes Beacon no vinculables al buscador | — | pendiente |
| NFR-RES-001 | MUST | F9 | Independencia de proveedores | — | pendiente |
| NFR-RES-002 | MUST | F6 | Degradación ante caída de Redis/Valkey | — | pendiente |
| NFR-RES-003 | MUST | F6 | Apagado ordenado | — | pendiente |
| NFR-MOB-001 | MUST | F7 | Matriz de dispositivos | — | pendiente |
| NFR-MOB-002 | SHOULD | F7 | Consumo de batería medido | — | pendiente |
| NFR-MOB-003 | SHOULD | E3 | Ahorro del muestreo adaptativo | — | pendiente |
| NFR-MOB-004 | MUST | E3 | Tamaño del núcleo nativo móvil | — | pendiente |
| NFR-MOB-005 | MUST | E3 | Tiempo de reacción a cambios de movimiento | — | pendiente |
| NFR-COMPAT-001 | MUST | F5 | Matriz de compatibilidad | — | pendiente |
| NFR-COMPAT-002 | MUST | F5 | Compatibilidad de contratos | — | pendiente |
| NFR-COMPAT-003 | MUST | E9 | Paridad de vectores entre lenguajes | — | pendiente |
| NFR-MAINT-001 | MUST | F0 | Versionado | `docs/templates/RELEASE_TEMPLATE.md`, `tools` (`geostream-tools changelog`) | cumplido |
| NFR-MAINT-002 | MUST | F1 | Cobertura de pruebas | `crates/gs-domain/tests/property_tests.rs`, `llvm-cov 90.76%` | cumplido |
| NFR-OBS-001 | MUST | F2 | Observabilidad estructurada | `crates/gs-observability/tests/logging_test.rs`, `crates/gs-observability/src/metrics.rs`, `apps/geostream/tests/cli_test.rs` | cumplido |
| NFR-OBS-002 | MUST | F2 | Cardinalidad acotada | `crates/gs-observability/src/metrics.rs` | cumplido |
| NFR-COST-001 | MUST | F6 | Operación sin proveedores comerciales | — | pendiente |
| NFR-DX-001 | MUST | F8 | Quickstart en menos de 30 minutos | — | pendiente |
| NFR-DOC-001 | MUST | F0 | Documentación completa | CI (`geostream-tools all`) | cumplido |
