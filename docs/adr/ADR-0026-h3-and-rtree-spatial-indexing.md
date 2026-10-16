# ADR-0026: H3 (`h3o`) para el índice vivo y los agregados; R-tree para polígonos

- **Estado:** Aceptado
- **Fecha:** 2026-10-15
- **Responsable:** propietario del proyecto
- **Requisitos afectados:** FR-POOL-003..006, FR-ANA-001, FR-ANA-004, FR-LRN-001, FR-GEO-004
- **Sustituye a:** ADR-0016 (mantiene su decisión de R-tree para polígonos de geocercas)

## Contexto

ADR-0016 eligió `rstar` (R-tree) para geocercas poligonales y difirió H3 porque en aquel momento no existían pools de disponibilidad con anillos de expansión, mapas de calor de densidad ni perfiles de velocidad por celda. Con las decisiones **D-07, D-08, D-09 y D-11**, el motor necesita:
1. Indexar decenas de miles de entidades en movimiento con consultas por radio y por anillos discretos crecientes (`rings` estilo InDrive).
2. Agregar actividad, velocidades y matrices origen-destino en celdas uniformes con k-anonimato y cuantización de posición para la vista de densidad (`density`).
3. Evaluar inclusión y distancia a polígonos arbitrarios de geocercas y aristas de grafos viales.

## Opciones consideradas

1. Usar solo R-tree para todo (ineficiente para anillos discretos, mapas de calor y agregaciones espaciales k-anónimas).
2. Usar la librería C de H3 vía FFI (introduce código C con `unsafe`, dificulta `no_std` y la compilación a `wasm32-unknown-unknown`).
3. **Combinar `h3o` (implementación de H3 en Rust 100 % puro y seguro, licencia BSD-3-Clause) bajo la feature `h3` de `gs-geo` para índices vivos por celdas, anillos y agregados analíticos, manteniendo `rstar` (R-tree) para objetos geométricos estáticos (polígonos de geocercas y aristas de red vial).**

## Decisión

Adoptar la Opción 3, sustituyendo parcialmente a ADR-0016:
- Conservar `rstar` (`RTreeIndex<T>` en `gs-geo`) para polígonos de geocercas (`14_GEOFENCING.md`) y segmentos de grafo vial.
- Aprobar la dependencia **`h3o`** en la lista aprobada de `19_ENGINEERING_STANDARDS.md` §6.2 bajo la feature `h3` de `gs-geo`.
- Usar H3 en:
  - `H3LiveIndex` (resolución `pool_index_resolution`, por defecto 9) para pools en memoria y búsqueda por `k`-anillos (`23_MOBILITY.md`).
  - Ofuscación espacial en `GET /v1/pools/{id}/density` (resolución `pool_density_resolution`).
  - Particionado espacial de historial (`h3_cell` en `location_points`), agregados horarios (`h3_activity_hourly`), matrices OD y perfiles de velocidad aprendidos (`24_HISTORY_ANALYTICS.md`).
- Las longitudes de arista y áreas de H3 se consultan a `h3o` en tiempo de ejecución; nunca se codifican constantes aproximadas a mano.

## Consecuencias

+ 100 % Rust seguro (`#![forbid(unsafe_code)]`), compatible con `no_std + alloc` y `wasm32-unknown-unknown`.
+ Licencia BSD-3-Clause compatible con `19` §6.1.
+ Estándar de la industria de movilidad para anillos de despacho y mapas de calor.

## Seguridad, privacidad y operación

Una celda H3 asociada a un individuo se trata como coordenada a efectos de redacción (`10` §8.2) y nunca se escribe en logs ni etiquetas de métricas Prometheus.

## Plan de migración

Se añade la feature `h3` en `crates/gs-geo` durante T-E1-05 con vectores dorados `GEO-H3`.

## Evidencia y referencias

- Decisiones D-07, D-08, D-09, D-11 en `docs/proposals/GeoStream_Addendum_A02_Motor_Ubicacion_Movilidad.md` §6.6, §9.2 y §17.4.
- `h3o` crate (BSD-3-Clause): https://lib.rs/crates/h3o (consultado 2026-10-15).
