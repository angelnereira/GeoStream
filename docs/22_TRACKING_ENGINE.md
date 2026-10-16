# 22 — Motor de trayectoria y muestreo

**ID:** GS-TRK-001 · **Versión:** 0.3.0
Gobierna `gs-track` (servidor) y la parte de muestreo de `gs-client-core` (dispositivo). Complementa `09` §4 (aceptación) y `08` (protocolo).

## 1. Alcance y límites

Este documento define el comportamiento del motor de trayectoria: la puerta de plausibilidad, el filtro de estimativa, el odómetro y la detección de paradas en el servidor, y el muestreo adaptativo, la publicación por cambio significativo, la cola sin conexión y la predicción visual en el cliente.

**Límites duros (principios P-E2, P-E3 y P-E4):**

1. El servidor **nunca** modifica, descarta ni sustituye una muestra `MEASURED` aceptada (`FR-LOC-006`). Todo lo derivado viaja en `LocationUpdate.estimate`, etiquetado con su `EstimateKind`.
2. Los algoritmos del camino caliente son *online*: estado O(1) por entidad y cero asignaciones de memoria por muestra.
3. Ningún algoritmo nuevo bloquea la aceptación ni el fan-out. El cómputo inline solo se permite dentro del presupuesto medido de §2.8; en caso contrario pasa a un canal acotado con descarte medido.

## 2. Motor de trayectoria en el servidor (`gs-track`)

### 2.1 Dónde vive el estado

- **Unidad de estimación: el dispositivo publicador.** Una muestra publicada en varias sesiones conserva la misma `sequence` (`12` §9), así que la estimación se calcula **una sola vez por `(device_id, sequence)`** y se reutiliza en todas las sesiones del lote.
- **Estado en memoria** en la instancia que posee la conexión. Por `FR-STR-007` hay una sola conexión por dispositivo.
- **Puntos de control** en el almacén efímero:
  - `{p}:trk:{device}` guarda el `TrackState` interno en Protobuf.
  - Se escribe al desconectar y cada `GS_TRACK_CHECKPOINT_INTERVAL_S`.
  - TTL `GS_TRACK_STATE_TTL_S`. Al reconectar en otra instancia, el estado se recupera de ahí.
  - La ingesta HTTP, que no tiene afinidad de instancia, usa un ciclo leer → actualizar → escribir con verificación de versión mediante `track_cas.lua`. Ante conflicto se reintenta una vez; si vuelve a fallar, se emite la actualización sin estimación y se incrementa `gs_track_state_conflict_total`.
- **Odómetro y paradas por `(sesión, miembro)`** en `{p}:odo:{session}:{member}` y `{p}:stop:{session}:{member}`. Se borran junto al snapshot cuando el miembro deja de compartir, sale o es revocado, y cuando la sesión termina (`04` §7.3–§7.5, `NFR-PRIV-002`).

### 2.2 Puerta de plausibilidad

Se evalúa **antes** del filtro sobre cada muestra aceptada. No rechaza ni modifica la muestra: produce banderas `PlausibilityFlag` y una puntuación `plausibility ∈ [0, 1]`.

Los nombres de esta tabla son **exactamente** los valores de `PlausibilityFlag` en `contracts/proto/geostream/v1/location.proto`, en su forma REST sin el prefijo `PLAUSIBILITY_FLAG_`. `geostream-tools phases` lo verifica: cambiar un nombre aquí sin cambiarlo en el contrato rompe la compilación.

| Bandera | Condición (parámetros en `17`) | Penalización |
|---|---|---|
| `EXCESSIVE_SPEED` | `d − k·(σ_prev + σ_new) > v_max(motion)·dt`, donde `d` es la distancia a la estimación previa en el marco ENU local, `σ` son las incertidumbres de posición, `k` es `track_accuracy_sigma_factor` y `v_max(motion)` sale de `track_max_speed_mps` y de la tabla por actividad. | 0,8 |
| `OUTLIER_JUMP` | La innovación del filtro supera la puerta de Mahalanobis (§2.3). | 0,6 |
| `MOCK_FLAGGED` | `quality.is_mock == true`. | 0,5 |
| `FROZEN_FIX` | Posición y precisión idénticas `track_frozen_fix_count` veces seguidas mientras avanza el tiempo y `motion ≠ STATIONARY`. | 0,5 |
| `IMPLAUSIBLE_ACCEL` | Con velocidades reportadas: `abs(s_new − s_prev)/dt > track_max_accel_mps2` más el margen de `speed_accuracy_mps`. | 0,4 |
| `LOW_ACCURACY` | `horizontal_accuracy_m > track_low_accuracy_m`. | 0,2 |
| `NETWORK_ONLY` | `quality.source == NETWORK`. | 0,1 |

Puntuación: `plausibility = max(0, 1 − Σ penalización(bandera))`. Las penalizaciones son constantes de `17` §2.11.

**Lo que NO es una bandera.** Un hueco temporal (`dt > track_reset_gap_s`) **no** es una bandera de plausibilidad: es un motivo de reinicio, y por eso no está en `PlausibilityFlag`. Va en `TrackResetReason` (§2.3), junto con `OUTLIER_RUN` y `FRAME_REANCHOR`. La razón es de comportamiento interno y no viaja en el contrato; las banderas sí, porque el integrador las necesita para tomar su decisión (D-10).

Las señales son **metadato de bajo nivel** (D-10): GeoStream no puntúa fraude, no lista ni sanciona. La decisión de negocio es del integrador.

### 2.3 Filtro de Kalman de velocidad constante

- **Estado:** `x = [e, n, v_e, v_n]` en el `LocalFrame` de la entidad (marco tangente equirectangular anclado en `(φ0, λ0)`, con `to_enu`/`from_enu`). `P` es 4×4. Se usan **arreglos fijos** `[f64; N]`: sin `nalgebra` y sin asignaciones por muestra.
- **Predicción:** `F(dt)` de velocidad constante; `Q(dt)` de aceleración ruido blanco discreta con `σ_a(motion)` de la tabla `GS_TRACK_PROCESS_NOISE_*`. Valores iniciales en m/s²: quieto 0,05; a pie 0,5; corriendo 1,0; bicicleta 1,0; automóvil 2,0; desconocido 1,5.
- **Medición de posición:** `z = to_enu(sample)`, `R = (track_accuracy_sigma_factor · horizontal_accuracy_m)² · I₂`.
- **Medición de velocidad** (opcional): se aplica cuando hay `speed_mps ≥ 1`, `heading_deg` y `speed_accuracy_mps`. `z_v = [s·sin θ, s·cos θ]`, con `R_v` derivado de las precisiones reportadas.
- **Puerta:** `d² = yᵀ S⁻¹ y` (inversa 2×2 cerrada). Si `d² > track_outlier_chi2` (inicial 13,82, χ² de 2 g.l. al 99,9 %), no se actualiza y se marca `OUTLIER_JUMP`. Tras `track_outlier_reset_count` outliers consecutivos, el filtro se reinicia en la nueva medición: así se recupera de saltos reales como la salida de un túnel o de un estacionamiento subterráneo.
- **Actualización:** forma de Joseph para estabilidad numérica.
- **Reinicio:** con `dt > track_reset_gap_s` o al re-anclar el marco (el marco se re-ancla cuando la entidad se aleja más de `track_frame_reanchor_m` de su origen). Motivos: `GAP`, `OUTLIER_RUN`, `FRAME_REANCHOR`, `FIRST_FIX`.
- **Estados:** `INIT`, `TRACKING`, `COASTING` (sin medición válida, solo predicción) y `RESET`.
- **Inicialización:** posición medida; velocidad reportada si existe, si no cero con `σ_v0 = 10 m/s`.

Pseudocódigo ilustrativo: `docs/proposals/GeoStream_Addendum_A02_Motor_Ubicacion_Movilidad.md` Anexo A.1.

### 2.4 Anti-deriva en reposo

Cuando la entidad está quieta (`motion == STATIONARY` reportado, o el detector de paradas en `STOPPED`, §2.6), se aplica una pseudo-medición de **velocidad cero** con `σ = GS_TRACK_ZUPT_SIGMA_MPS` (inicial 0,1 m/s). La posición converge a un promedio y no «camina» con el ruido del GPS. Es lo que evita que un repartidor detenido acumule metros fantasma (`NFR-QUAL-002`: deriva ≤ 5 m/h en reposo).

### 2.5 Odómetro en vivo

Distancia recorrida por miembro dentro de una sesión, sin guardar la ruta.

- **Método del ancla:** se mantiene un punto confirmado `A`. Cada estimación nueva `p` se compara con `A`. Si `fast_distance_m(A, p) ≥ track_odometer_noise_factor · position_std_m(p)`, se suma esa distancia y `A ← p`. Si no, no se suma nada; el movimiento lento acaba sumando porque `A` no se mueve hasta superar el umbral.
- **Exclusiones:** estimaciones con `EXCESSIVE_SPEED` o `OUTLIER_JUMP`, y estados distintos de `TRACKING`.
- **Huecos** (túneles, pérdida de señal): si `track_odometer_bridge_gaps = true` y la velocidad implícita del hueco es plausible, se suma la distancia recta y se contabiliza en `odometer_bridged_m`. Es una cota inferior honesta; la corrección por red vial llega con el map-matching (`24` §3).
- **Reinicio:** al activar la compartición en la sesión, y de forma explícita con `POST /v1/sessions/{session_id}/members/{member_id}/odometer/reset` (aplicación, scope `members:write`).

### 2.6 Paradas online

Versión en *streaming* de la detección de *stay points*. Memoria O(1): ancla, suma para el centroide, conteo e instante de inicio.

1. Si la nueva estimación está dentro de `r_eff = track_stop_radius_m + min(accuracy, track_stop_radius_m)` del centroide del candidato, se extiende el candidato.
2. Cuando el candidato dura `≥ track_stop_min_duration_s`, se emite `STOP_STARTED` con el centroide y `started_at`.
3. En estado `STOPPED`, `track_stop_exit_confirm` estimaciones consecutivas fuera de `r_eff` emiten `STOP_ENDED` con `duration_s`, y el candidato se reinicia en la última.

Precisión y recobrado objetivo: ≥ 0,95 (`NFR-QUAL-003`).

### 2.7 Salidas

- **`LocationEstimate`**, adjunta a `LocationUpdate.estimate` cuando la política `track_estimates_enabled` es verdadera:
  - posición estimada con `kind = FILTERED` (o `MAP_MATCHED`, `24` §3), donde `position.horizontal_accuracy_m` representa la desviación típica (1σ) de la posición estimada;
  - velocidad, rumbo y `position_std_m` implícito en la desviación;
  - `track_state` y `motion_state`;
  - `plausibility_flags` y `plausibility`;
  - `odometer_m`.
- **`TrackEvent`** por el stream (`ServerMessage.body = 28`, `request_id = 0`): `STOP_STARTED`, `STOP_ENDED`, `TRACKING_LOST` (paso a `COASTING` más allá de `track_lost_after_s`) y `TRACKING_RESUMED`. Solo `STOP_*` llevan `Position` (centroide), y **nunca en webhooks**.
- **Webhooks** `track.stop_started` y `track.stop_ended` **sin coordenadas** (mismo criterio que `FR-WHK-005`), con `session_id`, `member_id`, `occurred_at` y `duration_s`.

### 2.8 Presupuesto medido y degradación

- Histograma `gs_track_update_seconds` para la latencia por muestra; objetivo `NFR-PERF-005` (p99 ≤ 10 µs en el nodo de referencia).
- El objetivo de latencia interna `NFR-PERF-001` con el motor activo es `NFR-PERF-006` (p99 ≤ 50 ms en perfil P-M, sin cambios respecto a la línea base).
- Si la medición muestra que el presupuesto no se cumple, se abre un hallazgo (`20` §6, condición 5) y el cómputo de la estimación pasa a una tarea dedicada que emite la estimación en un mensaje aparte. **Nunca se degrada en silencio.**
- El snapshot guarda la actualización con su estimación, y las reglas de redacción de coordenadas (`10` §8) se aplican igual a la estimación.

## 3. Motor del dispositivo (`gs-client-core`)

### 3.1 Diseño sans-IO

El núcleo **no hace I/O**: recibe entradas y devuelve acciones (`26` §1). Por eso el mismo código funciona en Android, iOS, web (WASM), escritorio y backends, y se prueba de forma determinista con relojes simulados.

### 3.2 Muestreo adaptativo

Máquina de estados que combina tres señales: actividad del sistema operativo (si hay permiso), velocidad estimada por el filtro local y detector de paradas local.

- **Estados:** `STILL`, `ON_FOOT`, `CYCLING`, `VEHICLE` y `UNKNOWN`.
- **Histéresis:** subida rápida (una muestra con velocidad por encima del umbral, o actividad con confianza ≥ `sampling_activity_confidence`); bajada lenta (`sampling_downgrade_confirm_s`).
- **Intención** (`TrackingIntent`): el integrador declara qué está pasando, no cómo muestrear: `REALTIME_TRIP`, `REALTIME_IDLE`, `BACKGROUND_TRACKING` o `HISTORY_ONLY`.
- **Matriz inicial** de `LocationRequest` (valores iniciales; se calibran con `NFR-MOB-002` y viven en `17`):

| Intención · Estado | `STILL` | `ON_FOOT` | `CYCLING` / `VEHICLE` |
|---|---|---|---|
| `REALTIME_TRIP` | Equilibrada, 10 s, región de despertar | Alta, 3 s, 5 m | Alta, 1–2 s, 10 m |
| `REALTIME_IDLE` | Baja, 60 s, región de despertar | Equilibrada, 10 s, 20 m | Equilibrada, 5 s, 30 m |
| `BACKGROUND_TRACKING` | Pasiva y región de despertar | Equilibrada, 15 s, 25 m | Equilibrada, 10 s, 50 m |
| `HISTORY_ONLY` | Pasiva y región de despertar | Equilibrada, 30 s, lote de 5 min | Equilibrada, 10 s, lote de 2 min |

- `SamplingProfile` (`12` §11) se conserva como modo manual. Se añade `SamplingProfile.adaptive`, que pasa a ser el valor por defecto.
- **Activity Recognition** requiere permiso en ambas plataformas (Android `ACTIVITY_RECOGNITION` en tiempo de ejecución desde Android 10; iOS, descripción de uso de Core Motion). Sin permiso, el muestreo funciona solo con velocidad y detector de paradas.
- Objetivo de ahorro: `NFR-MOB-003` (≥ 25 % frente a `balanced` fijo en una jornada de reparto simulada), medido en `docs/device-tests/`.

### 3.3 Publicación por cambio significativo

Se publica la muestra `MEASURED` cuando se cumple **cualquiera** de estas condiciones, siempre respetando el piso `min_publish_interval_ms` del servidor:

1. distancia entre la estimación local actual y la última publicada `≥ max(publish_min_distance_m, publish_accuracy_factor · accuracy)`;
2. cambio de rumbo `≥ publish_min_course_change_deg` con velocidad `≥ publish_turn_min_speed_mps`;
3. cambio de velocidad `≥ publish_min_speed_change_mps`;
4. cambio de estado de movimiento o inicio/fin de parada;
5. han pasado `t_max` segundos, con `t_max = min(publish_max_silence_s, stale_after_seconds / 2)`, de modo que un suscriptor nunca ve como «stale» a quien sigue publicando.

Es la versión *online* de la simplificación de trayectorias: se envía menos sin perder la forma. Los giros se conservan por la regla 2. La muestra enviada es siempre `MEASURED` (P-E3): el filtro local decide **cuándo** enviar, no **qué**.

**Puerta local de calidad.** El dispositivo descarta antes de enviar solo los casos inequívocos: precisión ≤ 0 o NaN (ya en `12` §11) e `EXCESSIVE_SPEED` con incertidumbre previa baja. Cada descarte se cuenta en `invalid_samples_dropped`. Todo lo demás se envía y el servidor lo etiqueta.

### 3.4 Política de muestreo remota

Los umbrales de §3.2–§3.3 llegan en `ServerHello.sampling` (`SamplingPolicy`), con versión, desde la política del tenant (`sampling_policy`). Así un integrador ajusta el comportamiento de sus apps en campo sin publicar una versión nueva del SDK. Los defaults del SDK solo se usan antes de la primera conexión.

**Acotamiento obligatorio (T-30):** el núcleo acota cada valor recibido a los rangos duros de `17` antes de aplicarlo, e ignora valores ausentes o malformados. La política solo llega por el canal autenticado (`ServerHello` sobre TLS), nunca por un canal local manipulable sin autorización.

### 3.5 Cola sin conexión y entrega por lotes

Sustituye a ADR-0014 (ADR-0025).

- **`latestOnly`:** se mantiene para sesiones sin historial; conserva solo la última muestra por sesión y la descarta si supera `max_event_age_ms`.
- **`buffered`:** obligatorio cuando la sesión tiene historial (`history_mode ≠ OFF`, `24` §1):
  - cola cifrada en reposo con AES-256-GCM; la clave vive en el almacén seguro de la plataforma (Android Keystore, Keychain), expuesto por el puerto `SecureStore`;
  - límites `offline_buffer_max_samples` y `offline_buffer_max_age_s`;
  - al guardar se aplica la simplificación *online* por ventana (`offline_simplify_tolerance_m`, `gs-geo`);
  - al reconectar, lo más viejo que `max_event_age_ms` se sube por la **ingesta de historial** (`POST /v1/ingest/history`, `24` §2). Esa ruta escribe solo historial: no toca snapshot, fan-out ni secuencia viva. Lo reciente sigue el camino normal.
- **`HISTORY_ONLY`** usa la entrega por lotes del sistema operativo cuando existe (retardo máximo de actualización del proveedor de ubicación fusionado de Android) y sube cada `history_upload_interval_s` o al recuperar red.

Si el subject detiene la compartición o retira el consentimiento estando sin conexión, la cola `buffered` de esa sesión se purga y destruye de inmediato en el dispositivo.

### 3.6 Predicción visual (dead reckoning)

Enmienda `FR-MAP-005` (ADR-0030).

- **API:** `MemberView.renderPosition(now_ms)` devuelve `{position, kind, confidence_radius_m}` con `kind ∈ {MEASURED, FILTERED, PREDICTED}`. Los renderers de mapas (`13`) solo usan esta API.
- **Modelo:** `p(t) = p0 + v·τ·(1 − e^(−Δt/τ))` con `τ = predict_speed_decay_s`. La velocidad decae para no «salirse» de una curva o de un semáforo. `confidence_radius_m(t) = σ0 + predict_uncertainty_growth_mps · Δt`.
- **Sobre ruta:** si el integrador entrega una polilínea de ruta, la predicción avanza **sobre la polilínea** desde la proyección del último punto, sin pasar de su final.
- **Límites:** horizonte `predict_horizon_ms` (inicial 3000; máximo 10 000); desactivada si la entidad está quieta, si `plausibility < predict_min_plausibility`, si `freshness = stale` o si la velocidad es menor que `predict_min_speed_mps`.
- **Reconciliación:** al llegar una actualización real, el marcador converge en `predict_blend_ms` con suavizado críticamente amortiguado. Nunca hay salto brusco ni se dibuja hacia atrás.
- **Invariante:** las posiciones `PREDICTED` **nunca** salen del cliente. No se publican, no se guardan y no se registran.

### 3.7 Geocercas en el dispositivo y despertar por región

- **Al entrar en `STILL`:** el núcleo pide al anfitrión una región del sistema operativo (`wake_region_radius_m`, centrada en la parada) y baja el GPS a modo pasivo. Al salir de la región, el sistema despierta la app y el muestreo sube. Es la mayor palanca de batería en jornadas con muchas paradas.
- **Geocercas del integrador cercanas:** se cargan las N más próximas en los huecos de región que quedan libres. Las limitaciones de regiones por app en iOS y Android se verifican en la documentación oficial vigente. La evaluación exacta con histéresis la sigue haciendo el servidor (`14`).

### 3.8 Puertos de plataforma

`LocationProvider`, `ActivityProvider`, `RegionMonitor`, `BleScanner` (E8), `SecureStore`, `KeyValueStore`, `Connectivity`, `BackgroundScheduler`, `Clock` y `Logger` (redactado) — implementados por cada anfitrión (`26` §3).

## 4. Vectores dorados y escenarios

- **Vectores** en `contracts/test-vectors/`, cada uno con tolerancia declarada (absoluta o relativa) y **procedencia del valor esperado** (implementación de referencia independiente): `TRK-KF` (pasos del filtro con estado esperado), `TRK-PLAUS`, `TRK-ODO`, `TRK-STOP`, `SMP-DEC`, `PUB-SIG`, `PRD`.
- **Escenarios**: `SCN-014` (estimación adjunta con muestra intacta), `SCN-015` (paradas por stream y webhook sin coordenadas), `SCN-026` (`SamplingPolicy` remota aplicada y acotada).
- Los vectores se ejecutan en Rust y en cada binding (P-E6, `NFR-COMPAT-003`).
- Trayectorias sintéticas (`18` §3 y A-02 §16.2) mediante `gs-testkit::synthetic`: conducción urbana, reparto a pie, autopista, túnel, cañón urbano, GPS congelado, mock marcado, reposo con ruido y teletransporte. Nunca se usan coordenadas reales de personas (`11` §6).

## 5. Referencias

- `09` §4 (paso de estimación en la aceptación), `08` §6.1 (`ServerHello.sampling`) y §7 (`LocationUpdate`).
- `03` §2.19 (FR-TRK-*, FR-SMP-*), `03` §3 (NFR-PERF-005/006, NFR-QUAL-001..003, NFR-MOB-003..005).
- `17` §4 (política de trajectory y muestreo) y §5 (constantes del SDK).
- ADR-0022, ADR-0023, ADR-0025, ADR-0030.
