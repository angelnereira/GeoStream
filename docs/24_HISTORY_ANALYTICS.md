# 24 — Historial, capa aprendida y analítica

**ID:** GS-HAN-001 · **Versión:** 0.3.0
Gobierna `gs-analytics`, el escritor de historial y los trabajos por lotes. Complementa `11` (esquema y jobs) y `10` §7 (derechos del subject).

## 1. Política, propósito y consentimiento

El historial es una capacidad central del motor, pero **opt-in en tres llaves** (ADR-0024). El efímero por defecto se mantiene íntegro cuando las llaves no están activas.

**Tenant** (`17` §4):
- `history_allowed` pasa a estar activo (existe desde la línea base, reservado para F13).
- Claves nuevas: `history_raw_retention_days`, `history_trip_retention_days`, `analytics_allowed` y `analytics_k_anonymity`.

**Sesión:** `history_mode ∈ {OFF, TRACE, TRACE_AND_ANALYTICS}`, por defecto `OFF`. Lo fija la aplicación o el propietario con `manage`, y es visible en `SessionInfo.history_mode`.

**Membresía:** `consent_purposes ⊆ {HISTORY, ANALYTICS}`.
- Lo fija **solo el propio subject**, desde su dispositivo, al activar la compartición (`SetSharing` acepta `purposes`).
- **INV-SHR-5** (extensión de `INV-SHR-2`): ningún principal de aplicación ni miembro con `manage` puede otorgar, ampliar ni retirar un propósito de consentimiento en nombre de otro subject.
- Los cambios se auditan (`FR-PRV-005`, sin coordenadas).
- `LIVE` está implícito en `sharing = true` y no se registra como propósito.

**Reglas de escritura**

| Destino | Condiciones (todas necesarias) |
|---|---|
| Historial | `tenant.history_allowed` ∧ `session.history_mode ≠ OFF` ∧ `HISTORY ∈ member.consent_purposes` |
| Analítica | Todo lo anterior ∧ `tenant.analytics_allowed` ∧ `session.history_mode = TRACE_AND_ANALYTICS` ∧ `ANALYTICS ∈ member.consent_purposes` |

Los agregados excluyen siempre a quien no consintió `ANALYTICS`, aunque su historial esté activo.

**Retirar el consentimiento** detiene las escrituras de inmediato. Lo ya guardado sigue la retención o se borra a petición (§5).

## 2. Camino de escritura

- **Escritor por instancia** (roles `stream` y `api`):
  - canal acotado `GS_HISTORY_QUEUE_CAPACITY`;
  - lotes de `GS_HISTORY_BATCH_SIZE` o cada `GS_HISTORY_FLUSH_MS`;
  - inserción por COPY o INSERT multifila (el agente verifica la API de `sqlx` 0.9 antes de usarla, `20` §9);
  - deduplicación con `ON CONFLICT DO NOTHING` sobre la clave primaria `(tenant_id, device_id, sequence, captured_at)`.
- **Nunca bloquea la aceptación.** Si el canal se llena, se descarta y se cuenta en `gs_history_dropped_total`, con alerta. Objetivo de pérdida `NFR-PERF-009` (≤ 0,1 % sostenido en perfil P-M), medido con `gs-loadgen`.
- **Backfill:** `POST /v1/ingest/history` (dispositivo, scope `ingest:write`) recibe la cola `buffered` del cliente (`22` §3.5).
  - Aplica las reglas de `09` §3 salvo la antigüedad, ampliada hasta `history_backfill_max_age_s`.
  - Exige que la sesión esté activa, o cerrada hace menos de `history_backfill_grace_s`, y que el consentimiento siga vigente.
  - No toca la secuencia viva ni el snapshot; escribe solo historial.
  - Reutiliza la misma validación, la misma puerta de plausibilidad y el mismo filtro de trayectoria que el camino en vivo.

## 3. Analítica y capa de mapa aprendida

### 3.1 Trabajo de agregación

Todo lo de esta sección se calcula en el rol `analytics` (o `worker` con menor prioridad si ese rol no está activo), por lotes, con `FOR UPDATE SKIP LOCKED` para no competir por CPU con el rol `stream`.

| Producto | Cómo se obtiene | Para qué | Requisito |
|---|---|---|---|
| Historial de puntos | Escritor de §2 | Trazas, exportaciones | `FR-HIS-001` |
| Resumen de viajes (`trips`) | Douglas–Peucker (`trip_simplify_tolerance_m`), estadísticas, paradas offline, polilínea `gsp1` | Consulta de viajes, odómetro mejora | `FR-HIS-003` |
| Agregados horarios (`h3_activity_hourly`) | Celda H3 (`GS_HISTORY_H3_RESOLUTION`) × hora de la semana (168) × modo; histograma de velocidad de 1 m/s por cubeta | Mapas de calor, detección de congestión | `FR-ANA-001` |
| Perfiles de velocidad | Celda H3 (`learned_speed_resolution`, inicial 9) × hora de la semana × modo | ETA (`23` §4) | `FR-LRN-001` |
| Corrección de ETA | Residuo `duración real / estimada` por celda de origen y franja | ETA | `FR-LRN-001` |
| Factor de sinuosidad | `odómetro o distancia ajustada / haversine` de viajes terminados, por celda | ETA sin proveedor | `FR-LRN-001` |
| Lugares aprendidos | DBSCAN sobre centroides de paradas (`learned_places_eps_m`, `learned_places_min_points`), con k-anonimato de subjects distintos | Puntos sugeridos de recogida y entrega, entradas de edificios, puntos de espera | `FR-LRN-002` |
| Celdas de demanda y oferta | Conteos por celda y hora | Mapas de calor, reposicionamiento (lo decide el integrador) | `FR-ANA-001` |
| Matrices origen-destino | Celdas H3 de resolución `od_resolution` con k-anonimato | Análisis de flujos | `FR-ANA-004` |
| Eventos de conducción | Aceleraciones y frenadas bruscas estimadas desde la velocidad GNSS | Seguridad y analítica de flota | `FR-ANA-003` |

**Separación estricta de procedencia (ADR-0032):** la capa aprendida se construye **solo con datos GNSS propios del tenant**, nunca con contenido de proveedores. Así no se crea una base geográfica derivada de contenido ajeno (`docs/proposals/GeoStream_Addendum_A01_Integracion_Proveedores_Externos.md` §3.1).

**Reglas de los trabajos**
- El cierre de viaje se dispara cuando el miembro deja de compartir, sale, o la sesión se cierra o expira.
- La partición se crea con `GS_HISTORY_PARTITION_LOOKAHEAD_DAYS` de antelación.
- Las columnas nuevas de `sessions.history_mode` y `memberships.consent_purposes` se aplican por migración (`11` §2).

### 3.2 Eventos de conducción estimados

Se calculan al cerrar el viaje, solo cuando `speed_accuracy_mps` está presente y `dt ≤ 2 s`, contra los umbrales `driving_harsh_accel_mps2` / `driving_harsh_brake_mps2` (iniciales 3,0 y 3,5 m/s², a calibrar).

Se etiquetan **estimadas**: sin acelerómetro no son medidas. No se presentan como medición de aceleración del vehículo y no se publican en el stream.

### 3.3 Map-matching

Puerto `MapMatcher` con dos modos: `match_trace(trace) -> MatchedTrace` (lotes) y `MapMatcherSession` (online, retardo fijo).

- **Implementación de proveedor:** Valhalla (Meili), OSRM (`match`) o Google Roads, según manifiesto y términos (`13` §8).
- **Implementación integrada** (feature `own-graph`, `23` §7): HMM con Viterbi según Newson & Krumm (2009).
  - Emisión gaussiana sobre la distancia a cada candidato, con `σ_z = max(mm_sigma_z_m, accuracy)`.
  - Transición exponencial sobre `abs(distancia_por_ruta − distancia_ortodrómica)` con parámetro `mm_beta_m`.
  - Candidatos dentro de `mm_candidate_radius_m` (inicial 50), como máximo `mm_max_candidates` (inicial 8).
  - Distancia por ruta mediante Dijkstra acotado a `mm_max_route_factor` × ortodrómica + 200 m.
  - En lotes, Viterbi completo por viaje. Online, retardo fijo de `mm_fixed_lag` muestras (inicial 5).
  - Si no hay candidatos o la transición es imposible, la traza se corta en segmentos.
  - Los valores iniciales de `σ_z` y `β` se toman del artículo y **se calibran** con trazas propias; el agente verifica el artículo antes de fijarlos (`20` §9).

**Salidas**
- Polilínea ajustada a la vía, IDs de arista y distancia ajustada (`trip.distance_matched_m`, que mejora el odómetro).
- Velocidades por arista para la capa aprendida.
- En vivo, solo si la confianza supera `mm_live_min_confidence`, la estimación sale con `kind = MAP_MATCHED`.

## 4. API de consulta

| Endpoint | Quién | Notas |
|---|---|---|
| `GET /v1/sessions/{id}/members/{member_id}/trace` | Aplicación (`history:read`); dispositivo con `read` en la sesión, solo mientras está activa | `from`, `to`, `simplify_m`, `format ∈ {json, polyline, gsp1}` |
| `GET /v1/trips`, `GET /v1/trips/{trip_id}` | Aplicación (`history:read`) | Resumen y polilínea |
| `GET /v1/analytics/heatmap` | Aplicación (`analytics:read`) | `metric ∈ {presence, stops, speed}`, `resolution`, `from`, `to`, con k-anonimato |
| `GET /v1/analytics/od` | Aplicación (`analytics:read`) | Matrices origen-destino |
| `GET /v1/analytics/places` | Aplicación (`analytics:read`) | Lugares aprendidos cerca de un punto |
| `GET /v1/analytics/speed-profile` | Aplicación (`analytics:read`) | Por celda o caja y hora de la semana |
| `GET /v1/subjects/{id}/mobility-profile` | Aplicación (`analytics:read`) | Requiere consentimiento `ANALYTICS` vigente, si no `403 CONSENT_REQUIRED` |
| `POST /v1/exports`, `GET /v1/exports/{export_id}` | Aplicación (`history:read`) | Trabajo asíncrono en NDJSON o CSV con URL de descarga de TTL corto |
| `Subscribe.replay_trace` (stream) | Suscriptor con `read` | **COULD**: tras `SubscribeAck`, envía la ruta recorrida en la sesión activa como `LocationUpdate` con `delivery = REPLAY` (ya reservado) y simplificada |

## 5. Retención y supresión

- La retención se aplica **eliminando particiones** (puntos crudos, `history_raw_retention_days`) y con trabajos por lotes (viajes y paradas, `history_trip_retention_days`).
- `DELETE /v1/subjects/{id}` (`10` §7.2) se extiende a `location_points`, `trips`, `trip_tracks` y `stops` del subject, y a las claves efímeras `{p}:odo:*` y `{p}:stop:*` de sus membresías.
- Los agregados k-anónimos no contienen datos personales y se documentan como tales en `10` §6.2.
- La exportación del subject (`10` §7.3) incluye su historial, sus viajes y sus paradas.

## 6. k-anonimato

`analytics_k_anonymity` (inicial 5) es el umbral por debajo del cual **nada** se expone:

| Producto | Unidad de conteo |
|---|---|
| Mapa de calor | Subjects distintos que aportan muestras en la celda y la franja |
| Matriz OD | Subjects distintos que viajan en el par origen-destino |
| Perfil de velocidad | Subjects distintos que muestrearon la celda y hora |
| Lugar aprendido | Subjects distintos entre las paradas del cluster |

Una celda, un par o un lugar por debajo de *k* se suprime por completo: no se devuelve un valor agregado, ni degradado, ni redondeado.

## 7. Dimensionamiento

Fórmula de referencia para `16` (las cifras son ejemplos, **no medidas**):

```text
filas/día = entidades × (horas activas × 3600 / intervalo medio de publicación)
```

Por ejemplo, 10 000 conductores × 10 h × 1 muestra cada 5 s dan unos 72 millones de filas al día. Con unos 100 B por fila más índices, del orden de 7 GB/día de datos crudos. De ahí la retención corta de los puntos crudos y larga de los viajes compactados. El tamaño real por fila se mide en E6.

## 8. Referencias

- `11_DATA_STORAGE.md` §2 (esquema) y §5 (jobs), `10` §7 (derechos), `10` §4 (T-13, T-23, T-24).
- `03` §2.17 (FR-HIS-*, FR-ANA-*, FR-LRN-*), `03` §3 (NFR-PERF-009, NFR-PRIV-003).
- `17` §4 (política de historial y analítica) y §2 (parámetros del servidor).
- ADR-0024, ADR-0025, ADR-0032.
