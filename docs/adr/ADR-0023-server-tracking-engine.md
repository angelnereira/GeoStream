# ADR-0023: Motor de trayectoria en el servidor con estimación etiquetada

- **Estado:** Aceptado
- **Fecha:** 2026-10-15
- **Responsable:** propietario del proyecto
- **Requisitos afectados:** FR-LOC-006, FR-TRK-001..007, NFR-PERF-005, NFR-PERF-006, NFR-QUAL-001..003

## Contexto

Las muestras GNSS crudas en teléfonos reales sufren ruido térmico, saltos por multitrayecto en cañones urbanos, deriva en reposo (acumulando distancia falsa cuando el repartidor o conductor está detenido) y pérdidas temporales en túneles. Al mismo tiempo, la regla fundamental `FR-LOC-006` (principio `P-E3`) establece que el servidor nunca modifica ni descarta una muestra `MEASURED` aceptada.

## Opciones consideradas

1. Sobrescribir la posición de `LocationSample` en el servidor con la salida del filtro de Kalman (viola `FR-LOC-006` y destruye la trazabilidad de la medición).
2. Filtrar únicamente en el cliente antes de enviar (impide tener una estimación canónica uniforme para ingesta HTTP/LPWAN, odómetro de sesión auditable y detección de paradas en el servidor).
3. **Filtro dual complementario (D-04):** el dispositivo aplica puerta local de calidad y muestreo adaptativo, enviando siempre la muestra `MEASURED` intacta; el servidor (`gs-track`) calcula en streaming la estimación canónica (`LocationEstimate`, etiquetada `FILTERED` o `MAP_MATCHED`), banderas de plausibilidad, odómetro por `(sesión, miembro)` y eventos de parada (`TrackEvent`), adjuntándolos en un campo separado `LocationUpdate.estimate`.

## Decisión

Adoptar la Opción 3 (decisiones D-04 y D-10):
- Crear el crate puro `gs-track` sin I/O y con cero asignaciones dinámicas por muestra (matrices fijas 4×4 en `[f64; N]` sobre un marco tangente `LocalFrame` ENU de `gs-geo`).
- Ejecutar por dispositivo publicador y `sequence` (con caché por lote multi-sesión):
  1. **Puerta de plausibilidad** (§7.2): calcula 8 banderas (`PlausibilityFlag`) y puntuación `plausibility ∈ [0, 1]` como metadato para el integrador, sin rechazar la muestra.
  2. **Filtro de Kalman CV** (§7.3): estado `[e, n, v_e, v_n]`, actualización en forma de Joseph, puerta de Mahalanobis (`track_outlier_chi2`) y reinicio automático tras `track_outlier_reset_count` outliers consecutivos o salto temporal `track_reset_gap_s`.
  3. **Anti-deriva en reposo (ZUPT)** (§7.4): pseudo-medición de velocidad cero (`GS_TRACK_ZUPT_SIGMA_MPS`) cuando `motion == STATIONARY` o el detector de paradas está en `STOPPED`.
  4. **Odómetro en vivo por ancla** (§7.5) y **detector de paradas online O(1)** (§7.6) por `(sesión, miembro)`.
- Persistir puntos de control del estado del filtro en el almacén efímero (`{p}:trk:{device}`) al desconectar y cada `GS_TRACK_CHECKPOINT_INTERVAL_S`, y usar `track_cas.lua` en la ingesta HTTP.

## Consecuencias

+ Preserva `FR-LOC-006` al 100 %: la muestra medida viaja intacta y la estimación viaja etiquetada en `LocationUpdate.estimate`.
+ Estado O(1) por entidad y presupuesto medido (`NFR-PERF-005`: p99 ≤ 10 µs).
- Añade claves efímeras (`trk`, `odo`, `stop`) que deben limpiarse junto al snapshot al detener compartición, revocar o cerrar sesión.

## Seguridad, privacidad y operación

`LocationEstimate` y el centroide de `TrackEvent` contienen coordenadas y están sujetos a las mismas reglas de redacción (`10` §8.2) y borrado inmediato (`NFR-PRIV-002`) que `LocationSample`. Los webhooks `track.stop_started` y `track.stop_ended` nunca incluyen coordenadas (`FR-WHK-005`).

## Plan de migración

Aditivo en `location.proto` (`LocationEstimate`, `LocationUpdate.estimate = 6`) y `stream.proto` (`TrackEvent = 28`). Controlado por la política de tenant `track_estimates_enabled` (por defecto `true`).

## Evidencia y referencias

- Decisiones D-04 y D-10 en `docs/proposals/GeoStream_Addendum_A02_Motor_Ubicacion_Movilidad.md` §3 y §7.
- `docs/22_TRACKING_ENGINE.md`, `docs/09_VALIDATION_ERRORS.md` §4.
