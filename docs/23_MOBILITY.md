# 23 — Movilidad: pools, índice vivo y herramientas

**ID:** GS-MBL-001 · **Versión:** 0.3.0
Gobierna `gs-mobility` (librería pura) y el crate de adaptadores `gs-providers`. Complementa `05` §4.2 (modos de sesión) y `13` §8 (proveedores).

## 1. Modelo

El modelo se inspira en la práctica habitual del sector: por un lado, **ride-hailing con asignación** (candidatos por zona, ranking por tiempo de llegada y asignación por lotes); por otro, **ride-hailing con ofertas** al estilo InDrive (la solicitud se difunde a conductores cercanos por anillos crecientes y ellos responden).

GeoStream aporta las **primitivas**. La política (a quién ofrecer, cuánto cobrar, cómo resolver pujas) es del integrador (`01` NG-4).

### 1.1 Pools: un modo de sesión, no un concepto nuevo

`SessionMode.POOL` reutiliza toda la maquinaria existente: membresías, consentimiento, revocación en vivo, publicador único, secuencias, snapshot y consumidor del lado servidor. Es la formalización de UC-04 (Flota).

**Creación y lectura**
- Solo lo crea un principal de aplicación con scope `pools:manage`.
- Los miembros dispositivo tienen únicamente `publish` (**INV-POOL-1**: en un `POOL` ningún dispositivo tiene `read`, `invite` ni `manage`). Los conductores no se ven entre sí.
- Lo leen principales de aplicación: consultas con `pools:query` (§3) y, opcionalmente, el stream con `locations:read`.

**Límites y retención**
- `max_members` hasta `max_pool_members_cap` (política; valor inicial 100 000, a medir).
- `snapshot_ttl_seconds` por defecto: `pool_snapshot_ttl_default_seconds` (120).

**Consentimiento**
- «Ponerse en línea» = el propio dispositivo activa la compartición en el pool (`INV-SHR-1` no cambia).
- «Desconectarse» = dejar de compartir. La entrada del índice vivo se borra **antes de responder**, igual que el snapshot (`04` §7.4, `NFR-PRIV-002`).

**Disponibilidad** por miembro: `availability ∈ {AVAILABLE, BUSY, PAUSED}`. Estar fuera de línea es `sharing = false`.
- La aplicación la cambia con `PATCH /v1/sessions/{id}/members/{member_id}`.
- El propio dispositivo la cambia con `PUT /v1/sessions/{id}/members/me/availability`, scope `sharing:write` (es una acción del propio subject, como compartir).

**Atributos**
- Mapa de hasta `pool_max_attributes` entradas: clave `^[a-z0-9_]{1,32}$`, valor de hasta 64 caracteres. Por ejemplo `vehicle=moto`, `capacity=4`, `zone=norte`.
- Solo los escribe la aplicación.
- Se pueden filtrar por igualdad (hasta 3 a la vez).

**Coexistencia con viajes:** un dispositivo puede estar en un pool y en la sesión de su viaje a la vez. Publica la misma muestra (misma `sequence`) en ambas y el pool lo sigue mostrando como `BUSY`.

### 1.2 Patrón de «ventana de lotes»

El acumulador de solicitudes durante N segundos y la llamada a `/v1/tools/assignment` son del integrador; `15` lo documenta como patrón con un ejemplo ejecutable. GeoStream no mantiene ninguna cola de solicitudes en el servidor.

## 2. Índice espacial vivo

```rust
// ILUSTRATIVO
pub trait LiveIndex: Send + Sync {
    fn upsert(&self, pool: SessionId, member: MemberId, entry: LiveEntry);   // posición estimada, disponibilidad, atributos, ts
    fn remove(&self, pool: SessionId, member: MemberId);
    fn within(&self, pool: SessionId, q: &RadiusQuery) -> Vec<LiveHit>;     // radio + filtro + límite, ordenado por distancia
    fn rings(&self, pool: SessionId, q: &RingQuery) -> Vec<Vec<LiveHit>>;   // anillos crecientes
}
```

### 2.1 Implementaciones

| Implementación | Backend | Estructura | Uso |
|---|---|---|---|
| `H3LiveIndex` | Memoria (`GS_STATE_BACKEND=memory`) | `HashMap<CellIndex, SmallSet<MemberId>>` + `HashMap<MemberId, LiveEntry>`, resolución `pool_index_resolution` (inicial 9) | Un nodo |
| `ValkeyGeoIndex` | Redis/Valkey | `GEOADD {p}:pool:{session}:geo`, `HSET {p}:pool:{session}:meta:{member}`, `ZADD {p}:pool:{session}:seen`; alta y baja atómicas con `pool_upsert.lua`; consulta con `GEOSEARCH … BYRADIUS … ASC COUNT n` y filtrado de metadatos en *pipeline* | Multi-nodo |

**Ubicación de la implementación Valkey:** vive en `gs-ephemeral`, que pasa a depender de `gs-mobility` (crate puro) para implementar el trait (regla nueva en `04` §4.1) y la consume `gs-service` a través de ese trait.

### 2.2 Reglas comunes

- **Qué se indexa:** la **posición estimada** (`22` §2.7) cuando existe; si no, la medida. Así el ranking es más estable.
- **Escritura:** fuera del camino de `PublishAck`, por un canal acotado (`GS_POOL_INDEX_QUEUE`) con coalescencia por miembro (solo cuenta la última). Si se llena, se descarta y se cuenta en `gs_pool_index_updates_dropped_total`.
- **Caducidad:** las entradas con `now − ts > stale_after_seconds` se excluyen en la consulta. Un trabajo las poda cada `GS_POOL_PRUNE_INTERVAL_S`.
- **Borrado:** dejar de compartir, revocar o cerrar borra la entrada de forma **síncrona**, por privacidad.
- **Nunca** se escribe una coordenada en logs, métricas o trazas (`10` §8.2); tampoco una celda H3 de un individuo.

### 2.3 Presupuesto

`nearby` con 10 000 entidades, radio 3 km y límite 20 debe responder con p99 ≤ 2 ms en memoria y ≤ 10 ms con Valkey (`NFR-PERF-007`), medido con `criterion` y `gs-loadgen`.

## 3. Consultas

| Operación | Quién | Para qué |
|---|---|---|
| `POST /v1/pools/{session_id}/nearby` | Aplicación (`pools:query`) | Candidatos ordenados por distancia o por ETA |
| `POST /v1/pools/{session_id}/rings` | Aplicación (`pools:query`) | Candidatos agrupados por anillos crecientes para ofrecer un viaje de forma progresiva |
| `GET /v1/pools/{session_id}/density` | Dispositivo del cliente final (`pools:density`) o aplicación (`pools:query`) | «Autos alrededor» en la app del pasajero, **aproximados y anónimos** |

### 3.1 `nearby`

- Cuerpo: `center`, `radius_m` (≤ `pool_query_max_radius_m`), `limit` (≤ 100), `filter` (`availability[]`, `attributes`), `rank ∈ {distance, eta}` y `max_age_s`.
- Respuesta por candidato: `member_id`, `external_subject_id`, `distance_m`, `eta_s` y `eta_source` (si `rank=eta`), `age_ms`, la estimación (posición, velocidad, rumbo) y los atributos.
- Con `rank=eta` se toman los `pool_eta_prefilter` más cercanos por distancia y se refinan con §4. Así nunca se pide una matriz enorme al proveedor.

### 3.2 `rings`

- Cuerpo: `center`, `ring_step_m` (o `k` de H3), `max_rings`, `per_ring_limit` y `filter`.
- Devuelve los anillos en orden; el integrador decide cuándo pasar al siguiente.
- Los anillos son exactamente `k`-anillos H3: celdas a distancia exacta `k` del centro, lo que da anillos uniformes en toda la ciudad.

### 3.3 `density`

- Devuelve como máximo `pool_density_max_markers` marcadores, **sin IDs**.
- Cada posición se ajusta al centroide de su celda a la resolución `pool_density_resolution`, con desplazamiento aleatorio menor que media arista, y se retrasa al menos `pool_density_delay_s`.
- El rumbo es opcional y se cuantiza a 8 sectores.
- Sirve para mostrar actividad sin revelar dónde está cada conductor (T-25).

## 4. ETA

```text
// ILUSTRATIVO
eta_estimated_s = haversine(o, d) · circuity(cell(o), cell(d)) / speed(cell(o), hour_of_week, mode) + overhead_s(mode)
```

**Fuentes, de mayor a menor preferencia** (`source_preference` en la petición):

1. **Proveedor** (`RouteMatrixProvider` del gateway, `13` §8), si el tenant lo configuró con BYOK. Respeta cuotas y la política de caché del manifiesto.
2. **Capa aprendida** (`24` §3): velocidad por celda y franja horaria, y factor de corrección de residuos.
3. **Estimador por defecto:** `circuity = eta_default_circuity` (inicial 1,35) y velocidades por modo (iniciales: auto 6,9 m/s; moto 8,3; bicicleta 4,2; a pie 1,3). Valores iniciales **a calibrar con datos propios en E7**.

**Endpoint:** `POST /v1/tools/eta`, con `origins[] × destinations[]` (≤ `eta_max_matrix_cells`, inicial 625 = 25×25), `mode` y `departure_time`.
- Cada celda lleva `eta_s`, `distance_m` (si se conoce), `source ∈ {PROVIDER, LEARNED, ESTIMATED}` y `confidence`.
- Si el proveedor falla, se cae al siguiente nivel y se marca `source`. Si la petición exige `source_preference=provider` y no hay proveedor, responde `503 PROVIDER_UNAVAILABLE` con `Retry-After` (`09` §6).

## 5. Asignación por lotes

**Endpoint:** `POST /v1/tools/assignment`.
- Entrada: `workers[]` (id, posición o referencia a miembro de pool, `capacity`), `tasks[]` (id, posición), `cost ∈ {eta, distance, matrix}`, `max_cost`, `algorithm ∈ {hungarian, min_cost_flow}`.
- Salida: pares con su coste, tareas y trabajadores sin asignar, y coste total.

**Algoritmos** (implementados a mano en `gs-mobility`, sin dependencias):
- **Hungarian (Kuhn–Munkres)** para 1 a 1, rectangular, rellenando con `max_cost`. Objetivo ≤ 50 ms en 200 × 200 (`NFR-PERF-008`).
- **Flujo de coste mínimo** (caminos más cortos sucesivos con potenciales) cuando `capacity > 1`, por ejemplo un repartidor con dos pedidos.
- **Desempate determinista por ID**, para que las pruebas y los vectores `ASG` sean reproducibles.

**Límites y exposición**
- Tamaño ≤ `assignment_max_size` (inicial 500 × 500) en modo síncrono.
- La misma librería se expone en todos los bindings, para integradores que prefieran asignar en su backend.

## 6. Geocercas extendidas (F11 + E5)

- **`DWELL`:** nueva transición `GEOFENCE_TRANSITION_DWELL`. Se emite tras permanecer `INSIDE` durante `dwell_seconds` (atributo opcional de cada geocerca). Ejemplo: «el repartidor lleva 5 minutos en el restaurante».
- **Proximidad dinámica:** un círculo **anclado a un miembro** (`anchor_member_id`) de la misma sesión, con `GeofenceEvent.anchor_member_id`. Se evalúa cuando se mueve cualquiera de los dos y usa la misma histéresis de `14` §4. Ejemplo: «el conductor está a 300 m del pasajero». Límite: `max_dynamic_geofences_per_session`.
- **Evaluación en el dispositivo:** las geocercas cercanas ocupan huecos de región del sistema operativo (`22` §3.7) para despertar la app y ahorrar batería. La transición canónica la sigue decidiendo el servidor.

## 7. Grafo vial propio (opcional, por región)

- **Construcción:** herramienta `geostream graph build --osm <region>.osm.pbf --out <region>.gsg`, ejecutada **fuera de línea**. Genera grafo en formato CSR (nodos, aristas con longitud, clase de vía, sentido), R-tree de aristas y jerarquías de contracción (CH) si se piden.
- **Carga:** al arrancar, desde `GS_GRAPH_PATHS`. La memoria se mide y se informa en el benchmark de E7.
- **Habilita:** map-matching integrado (`24` §3) y, como **COULD**, `POST /v1/tools/route` con A* o CH (`source = OWN_GRAPH`) como alternativa al proveedor.
- **Licencia:** los datos OSM están bajo ODbL. La atribución es obligatoria (`13` §2). Antes de distribuir bases derivadas hace falta **revisión legal** de las obligaciones de *share-alike* (`10` §9).

## 8. Referencias

- `05` §4.2 y §5 (`POOL`, `INV-POOL-1`, disponibilidad, atributos), `07` §9 (operaciones).
- `03` §2.21 (FR-POOL-*, FR-MOB-*) y §2.15 (FR-GEO-005..007).
- `17` §4 (política de pools y herramientas).
- ADR-0026 (H3), ADR-0027 (pools), ADR-0028 (gateway BYOK), ADR-0032 (grafo propio).
