# 14 — Geocercas

**ID:** GS-GEO-001 · **Versión:** 0.2.0 · **Fase:** F11 · **Decisión:** ADR-0016
**Requisitos:** FR-GEO-001..004

## 1. Alcance
Detectar en el servidor cuándo un miembro que comparte entra o sale de una región, y notificarlo por stream y webhook. Se evalúa sobre muestras **aceptadas**; no requiere cambios en el SDK.

Fuera de alcance en v1: geocercas evaluadas en el dispositivo (más eficientes en batería; pista F15), "dwell"/permanencia como evento propio, consultas de proximidad entre miembros.

## 2. Modelo

| Campo | Regla |
|---|---|
| `id` | `geo_…` |
| `tenant_id` | del principal |
| `session_id` | opcional. Con valor: solo se evalúa para miembros de esa sesión. Sin valor: geocerca de tenant, se evalúa para todo miembro que comparte en cualquier sesión del tenant |
| `name` | 1..100 |
| `shape` | círculo o polígono (§3) |
| `metadata` | mismas reglas que la de sesión |

Quién puede crear: aplicación con `geofences:manage` (cualquier ámbito) o dispositivo con capacidad `manage` en la sesión (solo `session_id` de esa sesión).

Una misma muestra aceptada en dos sesiones se evalúa en el contexto de cada sesión; los eventos llevan `session_id`.

## 3. Validación de geometría

| Regla | Valor | Error |
|---|---|---|
| Círculo: `radius_m` | 25..=50000 | 422 `out_of_range` |
| Círculo: `center` | latitud/longitud válidas (`09` §3) | 422 |
| Polígono: `ring` | 4..=101 puntos; primero == último | 422 |
| Polígono: autointersecciones | prohibidas | 422 `invalid_format` |
| Polígono: área | > 0 | 422 |
| Polígono: extensión | caja envolvente ≤ 100 km por lado; no cruza el antimeridiano (diferencia absoluta de longitud ≤ 180° entre puntos consecutivos) | 422 |
| Límites | `max_geofences_per_tenant`, `max_geofences_per_session` (política) | 409 `CONFLICT` |

Orden de coordenadas en la API: objetos `{latitude, longitude}` (no arrays) para evitar la ambigüedad lat/lon.

## 4. Evaluación con histéresis

Parámetros de política: `geofence_confirm_samples` (default 2), `geofence_exit_buffer_m` (default 20), `geofence_max_accuracy_m` (default 100).

Para cada muestra aceptada `s` (precisión `a`) y cada geocerca candidata `g`:
1. Si `a > geofence_max_accuracy_m`: ignorar para transiciones (no altera contadores).
2. Calcular distancia firmada `d` en metros al borde (negativa dentro):
   - Círculo: `haversine(s, center) − radius_m`.
   - Polígono: proyección equirectangular local centrada en `s`; `d = −distancia_al_borde` si el punto está dentro, `+distancia_al_borde` si fuera.
3. Clasificar: `IN` si `d ≤ 0`; `OUT` si `d ≥ geofence_exit_buffer_m`; si no, `AMBIGUOUS` (no altera contadores).
4. Estado por (geocerca, miembro, sesión) en `{p}:geo:{geofence}:{member}`: `UNKNOWN` (inicial), `INSIDE`, `OUTSIDE`, con contador de clasificaciones consecutivas opuestas al estado.
5. Transiciones:
   - `UNKNOWN` → `INSIDE` tras `confirm_samples` clasificaciones `IN` consecutivas → **emite `ENTERED`** (permite detectar "ya llegó" aunque empiece a compartir en destino).
   - `UNKNOWN` → `OUTSIDE` tras `confirm_samples` `OUT` consecutivas → no emite.
   - `OUTSIDE` → `INSIDE` tras `confirm_samples` `IN` consecutivas → emite `ENTERED`.
   - `INSIDE` → `OUTSIDE` tras `confirm_samples` `OUT` consecutivas → emite `EXITED`.
6. El estado se borra cuando el miembro deja de compartir, sale o es revocado, o la sesión termina (sin emitir eventos).
7. `occurred_at` y `sequence` del evento = los de la muestra que confirma la transición.

## 5. Eventos
- Stream: `GeofenceEvent` a las suscripciones de la sesión con capacidad `read` y a consumidores de aplicación con `locations:read`.
- Webhooks: `geofence.entered` / `geofence.exited` sin coordenadas (FR-WHK-005).
- Auditoría: no (son eventos de datos, no acciones).

## 6. Indexación
- Por tenant, un R-tree en memoria (`rstar`) de cajas envolventes en grados, cargado desde PostgreSQL al arrancar el rol `stream` y actualizado al recibir `GeofencesChanged{tenant_id}` por `bus:global`.
- Consulta de candidatas: caja del punto expandida por `max(a, geofence_exit_buffer_m)` convertida a grados; luego prueba exacta (§4).
- La evaluación ocurre en una tarea separada con canal acotado; si el canal se llena, se descartan evaluaciones (métrica `gs_geofence_evaluations_dropped_total`) y **nunca** se retrasa la distribución de ubicaciones.
- H3 y Tile38 quedan diferidos: se adoptan solo con evidencia (memoria por instancia o latencia fuera de NFR-PERF-001 con los límites de política) y un ADR nuevo.

## 7. Pruebas
- Unitarias de geometría (bordes, polígonos cóncavos, cerca de los polos, cerca del antimeridiano rechazado).
- Trayectorias sintéticas con ruido gaussiano (σ = precisión declarada) que cruzan el borde: número exacto de transiciones esperado; sin oscilación dentro de la banda de histéresis (prueba de propiedades).
- Escenario de extremo a extremo con webhook.
- Benchmark con `max_geofences_per_tenant` geocercas.
