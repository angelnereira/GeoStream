# ADR-0032: Grafo vial propio opcional (OSM/ODbL) y map-matching integrado

- **Estado:** Aceptado
- **Fecha:** 2026-10-15
- **Responsable:** propietario del proyecto
- **Requisitos afectados:** FR-HIS-004, FR-LRN-001..004

## Contexto

Por la decisión **D-08** del propietario (`GS-ENG-A02` §3), GeoStream delega la red vial por defecto en el proveedor configurado (vía Provider Gateway, ADR-0028), pero no debe perder la capacidad de optimizar los mapas y rutas de las zonas que el sistema usa con frecuencia ni de operar de forma autónoma sin coste por llamada externa.

## Opciones consideradas

1. Depender exclusivamente de proveedores externos para todo map-matching y estimación de velocidades viales (costoso a gran escala y sujeto a restricciones de no derivación de bases de datos).
2. Almacenar y cachear indefinidamente las geometrías devueltas por Google Roads/Routes para construir un mapa propio (prohibido por los términos de servicio de Google Maps Platform verificados en A-01 §3.1).
3. **Combinar (a) una capa de mapa aprendida construida exclusivamente con telemetría GNSS propia del tenant (celdas H3 de velocidad, factores de sinuosidad, corrección de residuos de ETA y lugares aprendidos por DBSCAN) y (b) un grafo vial propio opcional por región (`geostream graph build --osm <region>.osm.pbf --out <region>.gsg`) con map-matching HMM/Viterbi integrado (Newson & Krumm, 2009).**

## Decisión

Adoptar la Opción 3 (decisión D-08):
- **Separación estricta de procedencia:** la capa aprendida (`24_HISTORY_ANALYTICS.md` §3) se calcula en el rol `analytics` **únicamente** a partir de muestras `MEASURED`/`FILTERED` propias del tenant con consentimiento `ANALYTICS`, nunca a partir de respuestas de proveedores comerciales.
- **Grafo vial propio opcional (`feature = "own-graph"`):**
  - La herramienta CLI `geostream graph build --osm <region>.osm.pbf --out <region>.gsg` compila fuera de línea un archivo compacto con el grafo dirigido en formato CSR (Compressed Sparse Row), un índice R-tree de aristas y jerarquías de contracción (CH) opcionales.
  - Al arrancar con `GS_GRAPH_PATHS`, el servidor carga los grafos regionales en memoria compartida de solo lectura.
- **Map-matching HMM/Viterbi integrado (`gs-analytics` / `gs-mobility`):** implementa el modelo oculto de Markov de Newson & Krumm (2009) con probabilidades de emisión gaussianas sobre distancia ortogonal a la arista y probabilidades de transición exponenciales sobre la diferencia entre distancia en red (Dijkstra acotado) y distancia ortodrómica, tanto por lotes (Viterbi completo al cerrar el viaje) como online con retardo fijo (`mm_fixed_lag`).

## Consecuencias

+ Permite mejorar progresivamente la precisión de ETA y odómetro en las ciudades donde opera el tenant sin infringir términos de terceros.
+ Permite hacer map-matching a coste marginal cero cuando se carga un grafo `.gsg` regional.
- Los datos de OpenStreetMap están licenciados bajo **ODbL**: requiere atribución visible (`© OpenStreetMap contributors`) y revisión legal de las obligaciones de *share-alike* antes de distribuir bases derivadas a terceros (registrado en `13_MAPS_PROVIDERS.md` §2 y `10_SECURITY_PRIVACY.md` §9).

## Seguridad, privacidad y operación

Todos los productos de la capa aprendida (perfiles de velocidad, lugares aprendidos, factores de sinuosidad) aplican obligatoriamente el umbral `analytics_k_anonymity` (`NFR-PRIV-003`).

## Plan de migración

Especificado en `23_MOBILITY.md` y `24_HISTORY_ANALYTICS.md` en E0; implementado en la fase E7 (`T-E7-01..08`).

## Evidencia y referencias

- Decisión D-08 en `docs/proposals/GeoStream_Addendum_A02_Motor_Ubicacion_Movilidad.md` §3 y §10.2–§10.4.
- Newson, P. & Krumm, J. (2009), *Hidden Markov Map Matching Through Noise and Sparseness*, ACM SIGSPATIAL GIS.
