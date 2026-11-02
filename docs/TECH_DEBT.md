# Deuda técnica y pendientes (hoja de ruta de cierre)

> Estado a 2026-11-01. Solo contiene lo **verificado** en el repositorio o en producción; cada punto dice cómo se comprobó. No sustituye a `21_ROADMAP.md` (que define las tareas) ni a `PROGRESS.md` (que registra lo hecho): ordena lo que falta por riesgo y dice quién tiene que mover cada cosa.

## 1. Bloqueado por una decisión o una acción del propietario

| # | Pendiente | Por qué espera | Qué se necesita |
|---|---|---|---|
| D-1 | Rotar la clave de Resend | Se pegó en un chat; hay que tratarla como expuesta | Crear una nueva en Resend y ponerla en `GS_CONSOLE_EMAIL_API_KEY` (Railway) |
| D-2 | Auditoría de la consola (FR-CON-008) | Las funciones de `admin` registran `actor_kind = OPERATOR`; la consola necesita un actor propio, y eso cambia el esquema | Autorizar el tipo de actor nuevo |
| D-3 | `GET /v1/status` público | Cambia el contrato (`DR-20261101-03`) | Elegir la opción 2 o 3 |
| D-4 | Primera etiqueta semver (`v0.1.0`) y releases | `Cargo.toml` está en `0.0.0` y no hay tags | Aprobar el número |
| D-5 | Licencia | Sin ella no se pueden publicar SDK ni descargas | Decisión del propietario |
| D-6 | Retención de posiciones para Plenty-Pay (Q-49) | La fija el negocio. Hoy las posiciones en vivo son efímeras y el historial solo existe si el tenant lo activa | Plazo, o confirmar que no se activa historial |
| D-7 | Registros DNS de Squarespace | El TXT `_railway-verify.console.geostream` llegó con un punto de más | Dejar un solo TXT sin el punto final |
| D-8 | `--save` de Redis en Railway | Persistencia del almacén efímero; nunca se confirmó | Decidir si se activa |
| D-9 | GitHub Actions detenido por facturación | **No hay CI** desde 2026-10-20: ningún PR se verifica automáticamente | Resolver la facturación |

## 2. Riesgo de corrección en producción (hacer primero)

| # | Pendiente | Evidencia | Estado |
|---|---|---|---|
| R-1 | El rol `worker` no hacía nada: las sesiones nunca pasaban a `EXPIRED` por tiempo y no se purgaba nada | `gs-worker/src/lib.rs` tenía 2 líneas; `expiry_loop` no tenía ningún llamador | **Corregido** (este PR): barrido de expiración y purga horaria de idempotencia y datos vencidos de la consola |
| R-2 | `POST /v1/ingest/locations` no funcionaba con el contrato (enums y antigüedad 0) | Reproducido con `curl`; ninguna prueba HTTP lo cubría | **Corregido** (PR #30) |
| R-3 | No había una comprobación automática de que cada operación del OpenAPI tuviera una prueba. R-2 pasó inadvertido por eso | `geostream-tools op-coverage` (trinquete) | **Parcial** (este PR): una operación nueva sin prueba falla. Quedan 44 operaciones en `tools/op_coverage_baseline.txt` (pools, analytics, historial, edge, tools, tags, webhooks, geocercas…): hay que cubrirlas hasta vaciar la lista |
| R-4 | Métricas de los jobs del worker y de fallos de envío de correo | Se descubrió el 403 de Resend solo por el log | **Parcial** (este PR): `gs_worker_job_duration_seconds`, `gs_worker_job_rows_total`, `gs_worker_job_failures_total` (probadas) y `gs_console_mail_failed_total` (instrumentada, sin prueba propia). Falta definir y desplegar la alerta |
| R-5 | `FR-SES-005` figuraba «cumplido» sin que nada ejecutara la expiración | `TRACEABILITY.md` citaba solo pruebas del caso de uso | **Corregido** (este PR): la fila cita la prueba del worker |
| R-6 | El límite por IP del registro depende de `X-Forwarded-For` | nginx del portal lo sustituye por la última entrada; **una llamada directa a la API** con la cabecera falsificada lo evade | Abierto: ADR sobre qué proxies son de confianza |
| R-7 | `Idempotency-Key` se ignora en silencio si no hay clave de cifrado de datos | Mismo comportamiento que `/v1/sessions` | Abierto: rechazar con error en lugar de ignorar |

## 3. Retención y limpieza que `11` §5 promete

Ya hace el worker: expiración de sesiones, idempotencia, datos vencidos de la consola y, una vez al día desde `GS_WORKER_RETENTION_HOUR_UTC`, el borrado por lotes (`FOR UPDATE SKIP LOCKED`) de sesiones cerradas o expiradas pasados `GS_RETENTION_CLOSED_SESSIONS_D` (90 d, con cascada a miembros e invitaciones) y de invitaciones caducadas o revocadas pasados `GS_RETENTION_INVITATIONS_D` (30 d). Una sesión activa nunca se borra.

Sigue sin implementar:
- Auditoría (`GS_RETENTION_AUDIT_D`, 365 d) y entregas de webhooks (`GS_RETENTION_WEBHOOK_DELIVERIES_D`; los webhooks son F8).
- Poda del índice de pools, expiración de reportes edge, particiones e historial (fase E6).
- Los purgados de idempotencia y de la consola siguen siendo `DELETE` sin límite de lote (correctos, pero con tablas grandes conviene acotarlos).

## 4. Producto que falta

| Prioridad | Qué | Tarea | Nota |
|---|---|---|---|
| 1 | Seguimiento en tiempo real por WebSocket para el cliente final | T-F8-05, `DR-20261102-01` | La opción 1 (relevo del snapshot, `examples/tracking-relay-node`) ya está **decidida y disponible**; esto es el paso a tiempo real, con Dart generado desde Protobuf |
| 2 | Webhooks | T-F8-01 | Outbox transaccional, firma, SSRF. Habilita «llegó» por webhook |
| 3 | Geocercas en el servidor | T-F11-* | Hoy se evalúan en el cliente |
| 4 | Panel de uso y cuotas | FR-CON-006, FR-CON-007 | |
| 5 | Degradación ordenada si Redis cae | T-F6-03 (resto) | `04` §7.6 |
| 6 | Drenaje en SIGTERM | T-F6-04 | Sin él, un despliegue corta conexiones abiertas |
| 7 | CORS por aplicación | T-F8-04 / T-F15-11 | Requiere una decisión de seguridad |
| 8 | «Entrar con Gravital» | T-F15-20 | Depende de que Plenty-Pay publique OIDC con claves asimétricas |
| 9 | Cuenta de consola separada de los usuarios finales (FR-CON-002) y reglas del usuario sobre su ubicación (FR-PRT-006) | | Sin tests ni implementación específica |

## 5. Verificaciones que no se han hecho
- La imagen Docker completa del portal no se reconstruyó tras añadir la consola (sí se probó nginx real con la configuración renderizada).
- La navegación landing → consola en un navegador real: solo cubierta por pruebas de widgets.
- Un envío real de correo de punta a punta con una persona distinta del propietario.
- Latencia de ingesta y snapshot frente al objetivo de p95 < 300 ms (NFR); no se ha medido.

## 6. Higiene
- Ramas remotas ya superadas, sin borrar: `feat/T-F6-01-redis-backends`, `feat/T-F6-02-cross-instance-supersede`, `feat/T-F6-03-readiness-state`, `feat/T-F15-10-own-tiles`.
- Disco local al 99 %: `target/` ocupa decenas de GB; limpiar con `cargo clean` cuando no se esté compilando.
- El dominio `gravitalcloud.com` en Resend figura «partially_failed» solo por el MX de recepción; no afecta al envío.
