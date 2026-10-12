# Trazabilidad de requisitos

**ID:** GS-TRC-001 · Generado inicialmente desde `03_REQUIREMENTS.md` (v0.2.0).

Reglas:
- Una fila por requisito, en el orden de `03`. Nunca se borran filas (un requisito retirado se marca `RETIRADO`).
- La columna **Pruebas** se regenera con el script de `tools/` a partir de los `covers:` de las pruebas (`18` §11); hasta que exista, la mantienen los agentes a mano.
- **Estado**: `pendiente` → `en curso` → `implementado` (código + pruebas verdes) → `verificado` (incluido en un release o hito con evidencia).
- CI falla si un requisito MUST de una fase cerrada no está `implementado` con pruebas.

| Requisito | Prioridad | Fase | Título | Pruebas | Estado |
|---|---|---|---|---|---|
| FR-AUTH-001 | MUST | F2 | API keys de aplicación | — | pendiente |
| FR-AUTH-002 | MUST | F2 | Emisión de tokens de dispositivo | — | pendiente |
| FR-AUTH-003 | MUST | F2 | Verificación de tokens | — | pendiente |
| FR-AUTH-004 | MUST | F2 | Rotación de claves de firma | — | pendiente |
| FR-AUTH-005 | MUST | F3 | Revocación de tokens y desactivación de dispositivos | — | pendiente |
| FR-AUTH-006 | MUST | F4 | Tickets de stream de un solo uso | — | pendiente |
| FR-AUTH-007 | MUST | F4 | Renovación de autenticación dentro de la conexión | — | pendiente |
| FR-AUTH-008 | MUST | F2 | Scopes | — | pendiente |
| FR-AUTH-009 | COULD | F15 | Tokens firmados por el integrador | — | pendiente |
| FR-ADM-001 | MUST | F2 | CLI de administración | — | pendiente |
| FR-ADM-002 | MUST | F2 | Políticas por tenant | — | pendiente |
| FR-ADM-003 | MUST | F3 | Deshabilitar tenant o aplicación | — | pendiente |
| FR-ADM-004 | COULD | F15 | API HTTP de administración | — | pendiente |
| FR-API-001 | MUST | F3 | Errores Problem Details | — | pendiente |
| FR-API-002 | MUST | F3 | Idempotencia | — | pendiente |
| FR-API-003 | MUST | F3 | Paginación por cursor | — | pendiente |
| FR-API-004 | MUST | F3 | Rate limit REST | — | pendiente |
| FR-API-005 | MUST | F3 | OpenAPI como fuente de verdad | — | pendiente |
| FR-API-006 | MUST | F3 | No enumeración de recursos | — | pendiente |
| FR-SES-001 | MUST | F3 | Crear sesión | — | pendiente |
| FR-SES-002 | MUST | F3 | Consultar y listar sesiones | — | pendiente |
| FR-SES-003 | MUST | F3 | Actualizar sesión | — | pendiente |
| FR-SES-004 | MUST | F3 | Cerrar sesión | — | pendiente |
| FR-SES-005 | MUST | F3 | Expiración automática | — | pendiente |
| FR-SES-006 | MUST | F3 | Modos de sesión | — | pendiente |
| FR-SES-007 | MUST | F3 | Salida del propietario | — | pendiente |
| FR-MEM-001 | MUST | F3 | Agregar miembros (aplicación) | — | pendiente |
| FR-MEM-002 | MUST | F3 | Listar y consultar miembros | — | pendiente |
| FR-MEM-003 | MUST | F3 | Cambiar capacidades | — | pendiente |
| FR-MEM-004 | MUST | F3 | Revocar miembro | — | pendiente |
| FR-MEM-005 | MUST | F3 | Abandonar sesión | — | pendiente |
| FR-MEM-006 | MUST | F3 | Límite de miembros | — | pendiente |
| FR-MEM-007 | MUST | F3 | Readmisión | — | pendiente |
| FR-INV-001 | MUST | F3 | Crear invitación | — | pendiente |
| FR-INV-002 | MUST | F3 | Canjear invitación | — | pendiente |
| FR-INV-003 | MUST | F3 | Listar y revocar invitaciones | — | pendiente |
| FR-INV-004 | COULD | F15 | Aprobación de solicitudes de unión | — | pendiente |
| FR-SHR-001 | MUST | F3 | Apagado por defecto | — | pendiente |
| FR-SHR-002 | MUST | F3 | Activación solo por el propio subject | — | pendiente |
| FR-SHR-003 | MUST | F3 | Detener compartición | — | pendiente |
| FR-SHR-004 | MUST | F3 | Un único dispositivo publicador | — | pendiente |
| FR-SHR-005 | MUST | F3 | Auditoría de compartición | — | pendiente |
| FR-LOC-001 | MUST | F4 | Modelo de muestra | — | pendiente |
| FR-LOC-002 | MUST | F4 | Validación de campos | — | pendiente |
| FR-LOC-003 | MUST | F4 | Orden y deduplicación | — | pendiente |
| FR-LOC-004 | MUST | F4 | Validación temporal | — | pendiente |
| FR-LOC-005 | MUST | F4 | Tiempo de recepción del servidor | — | pendiente |
| FR-LOC-006 | MUST | F4 | Calidad y tipo de dato | — | pendiente |
| FR-LOC-007 | MUST | F4 | Rate limit de publicación | — | pendiente |
| FR-LOC-008 | MUST | F4 | Sequence floor | — | pendiente |
| FR-LOC-009 | MUST | F8 | Ingesta HTTP por lotes | — | pendiente |
| FR-STR-001 | MUST | F4 | Endpoint y handshake | — | pendiente |
| FR-STR-002 | MUST | F4 | Suscripción | — | pendiente |
| FR-STR-003 | MUST | F4 | Distribución autorizada | — | pendiente |
| FR-STR-004 | MUST | F4 | Acks de publicación | — | pendiente |
| FR-STR-005 | MUST | F4 | Heartbeat, inactividad y cierre | — | pendiente |
| FR-STR-006 | MUST | F4 | Backpressure | — | pendiente |
| FR-STR-007 | MUST | F4 | Una conexión por dispositivo | — | pendiente |
| FR-STR-008 | MUST | F4 | Revocación en vivo | — | pendiente |
| FR-STR-009 | MUST | F6 | Fan-out multi-instancia | — | pendiente |
| FR-STR-010 | MUST | F6 | Drenaje controlado | — | pendiente |
| FR-STR-011 | MUST | F4 | Eventos de sesión y miembros | — | pendiente |
| FR-PRS-001 | MUST | F4 | Estado de miembro | — | pendiente |
| FR-PRS-002 | MUST | F4 | Gracia de desconexión | — | pendiente |
| FR-PRS-003 | MUST | F6 | Presencia multi-instancia | — | pendiente |
| FR-PRS-004 | MUST | F4 | Minimización de presencia | — | pendiente |
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
| FR-SDK-013 | MUST | F5 | App de ejemplo | — | pendiente |
| FR-SDK-014 | SHOULD | F8 | Flutter Web | — | pendiente |
| FR-SDK-015 | MUST | F5 | Inyección de dependencias | — | pendiente |
| FR-SDK-016 | MUST | F5 | Núcleo sin mapas | — | pendiente |
| FR-MAP-001 | MUST | F9 | Interfaces de adaptador | — | pendiente |
| FR-MAP-002 | MUST | F9 | Stacks de proveedor | — | pendiente |
| FR-MAP-003 | MUST | F9 | Adaptador MapLibre | — | pendiente |
| FR-MAP-004 | SHOULD | F9 | Adaptador Google Maps | — | pendiente |
| FR-MAP-005 | MUST | F9 | Interpolación visual etiquetada | — | pendiente |
| FR-MAP-006 | MUST | F9 | Documentación de licencias | — | pendiente |
| FR-WHK-001 | MUST | F8 | Registro de endpoints | — | pendiente |
| FR-WHK-002 | MUST | F8 | Outbox y entrega at-least-once | — | pendiente |
| FR-WHK-003 | MUST | F8 | Firma | — | pendiente |
| FR-WHK-004 | MUST | F8 | Protección SSRF | — | pendiente |
| FR-WHK-005 | MUST | F8 | Sin coordenadas | — | pendiente |
| FR-INT-001 | MUST | F8 | Consumidor del lado servidor | — | pendiente |
| FR-INT-002 | MUST | F4 | Snapshot REST | — | pendiente |
| FR-INT-003 | SHOULD | F8 | CORS por aplicación | — | pendiente |
| FR-INT-004 | MUST | F8 | Guía de integración | — | pendiente |
| FR-GEO-001 | MUST | F11 | Gestión de geocercas | — | pendiente |
| FR-GEO-002 | MUST | F11 | Evaluación con histéresis | — | pendiente |
| FR-GEO-003 | MUST | F11 | Eventos | — | pendiente |
| FR-GEO-004 | SHOULD | F11 | Índice espacial en proceso | — | pendiente |
| FR-PRV-001 | MUST | F4 | Efímero por defecto | — | pendiente |
| FR-PRV-002 | MUST | F10 | Borrado y exportación de subject | — | pendiente |
| FR-PRV-003 | MUST | F10 | Jobs de retención | — | pendiente |
| FR-PRV-004 | MUST | F4 | Redacción de coordenadas | — | pendiente |
| FR-PRV-005 | MUST | F3 | Auditoría | — | pendiente |
| FR-HIS-001 | COULD | F13 | Historial opt-in | — | pendiente |
| FR-HIS-002 | COULD | F13 | Cola offline acumulada | — | pendiente |
| FR-HIS-003 | COULD | F13 | Consulta de historial | — | pendiente |
| FR-HIS-004 | COULD | F13 | Calidad de trayectoria y map-matching | — | pendiente |
| FR-OPS-001 | MUST | F0 | Binario único con roles | tests/cli_test.rs | cumplido |
| FR-OPS-002 | MUST | F2 | Salud y métricas | — | pendiente |
| FR-OPS-003 | MUST | F1 | Configuración validada | — | pendiente |
| FR-OPS-004 | MUST | F2 | Migraciones versionadas | — | pendiente |
| FR-OPS-005 | MUST | F6 | Imagen y compose | — | pendiente |
| FR-OPS-006 | MUST | F12 | Runbooks y backups | — | pendiente |
| FR-IOT-001 | COULD | F14 | Puente MQTT | — | pendiente |
| FR-NAT-001 | COULD | F15 | SDKs nativos Kotlin/Swift | — | pendiente |
| FR-E2E-001 | COULD | F16 | Modo cifrado de extremo a extremo | — | pendiente |
| FR-P2P-001 | COULD | F16 | Transporte P2P WebRTC | — | pendiente |
| NFR-PERF-001 | MUST | F6 | Latencia interna del servidor | — | pendiente |
| NFR-PERF-002 | MUST | F6 | Latencia extremo a extremo | — | pendiente |
| NFR-PERF-003 | MUST | F6 | Capacidad de un nodo de referencia | — | pendiente |
| NFR-PERF-004 | SHOULD | F6 | Memoria por conexión | — | pendiente |
| NFR-SCALE-001 | MUST | F6 | Escalado horizontal | — | pendiente |
| NFR-SCALE-002 | MUST | F6 | Tormenta de reconexión | — | pendiente |
| NFR-AVAIL-001 | SHOULD | F12 | Objetivo de disponibilidad | — | pendiente |
| NFR-SEC-001 | MUST | F2 | Transporte cifrado | — | pendiente |
| NFR-SEC-002 | MUST | F4 | Plazo de revocación | — | pendiente |
| NFR-SEC-003 | MUST | F2 | Secretos fuera de registros | — | pendiente |
| NFR-SEC-004 | MUST | F3 | Aislamiento de tenant | — | pendiente |
| NFR-SEC-005 | MUST | F10 | Fuzzing | — | pendiente |
| NFR-SEC-006 | MUST | F0 | Auditoría de dependencias | CI (cargo deny, cargo audit) | cumplido |
| NFR-PRIV-001 | MUST | F4 | Sin persistencia de coordenadas | — | pendiente |
| NFR-PRIV-002 | MUST | F4 | Borrado de snapshot inmediato | — | pendiente |
| NFR-RES-001 | MUST | F9 | Independencia de proveedores | — | pendiente |
| NFR-RES-002 | MUST | F6 | Degradación ante caída de Redis/Valkey | — | pendiente |
| NFR-RES-003 | MUST | F6 | Apagado ordenado | — | pendiente |
| NFR-MOB-001 | MUST | F7 | Matriz de dispositivos | — | pendiente |
| NFR-MOB-002 | SHOULD | F7 | Consumo de batería medido | — | pendiente |
| NFR-COMPAT-001 | MUST | F5 | Matriz de compatibilidad | — | pendiente |
| NFR-COMPAT-002 | MUST | F5 | Compatibilidad de contratos | — | pendiente |
| NFR-MAINT-001 | MUST | F0 | Versionado | — | pendiente |
| NFR-MAINT-002 | MUST | F1 | Cobertura de pruebas | — | pendiente |
| NFR-OBS-001 | MUST | F2 | Observabilidad estructurada | — | pendiente |
| NFR-OBS-002 | MUST | F2 | Cardinalidad acotada | — | pendiente |
| NFR-COST-001 | MUST | F6 | Operación sin proveedores comerciales | — | pendiente |
| NFR-DX-001 | MUST | F8 | Quickstart en menos de 30 minutos | — | pendiente |
| NFR-DOC-001 | MUST | F0 | Documentación completa | CI (tools/run_all_checks.py) | cumplido |
