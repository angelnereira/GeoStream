# ADR-0027: Pools como modo de sesión e índice vivo con dos backends

- **Estado:** Aceptado
- **Fecha:** 2026-10-15
- **Responsable:** propietario del proyecto
- **Requisitos afectados:** FR-POOL-001..006, FR-MOB-001..002, NFR-PERF-007, NFR-PERF-008

## Contexto

Las plataformas de ride-hailing (estilo Uber o InDrive) y de reparto necesitan mantener un conjunto de conductores o repartidores en línea («pool»), consultar candidatos cercanos ordenados por distancia o ETA (`nearby`), difundir solicitudes por anillos crecientes (`rings`), mostrar densidad aproximada y anónima en la app del pasajero (`density`) y resolver asignaciones por lotes (`assignment`). Crear una entidad paralela desconectada de las sesiones duplicaría membresías, consentimiento, revocación en vivo, publicador único y secuencias.

## Opciones consideradas

1. Crear un subsistema de «conductores disponibles» separado de las sesiones.
2. **Modelar un pool como `SessionMode.POOL` sobre las sesiones existentes, gobernado por la invariante `INV-POOL-1` y respaldado por el trait `LiveIndex` con dos implementaciones (`H3LiveIndex` en memoria y `ValkeyGeoIndex` en Redis/Valkey).**

## Decisión

Adoptar la Opción 2 (decisión D-09):
- **Modo `SessionMode.POOL`:** solo lo crea una aplicación con scope `pools:manage`.
- **Invariante `INV-POOL-1`:** en una sesión `POOL`, ningún miembro dispositivo posee las capacidades `read`, `invite` ni `manage` (solo `publish`). Los conductores nunca se ven entre sí.
- **Consentimiento e intimidad:** ponerse en línea equivale a que el propio subject active `sharing = true` (`INV-SHR-1` y `INV-SHR-2` intactas). Desconectarse (`sharing = false`), salir, ser revocado o cerrar el pool elimina la entrada del `LiveIndex` de forma **síncrona antes de responder** (`NFR-PRIV-002`).
- **Disponibilidad y atributos:** cada miembro tiene `availability ∈ {AVAILABLE, BUSY, PAUSED}` y hasta `pool_max_attributes` atributos clave-valor filtrables por igualdad, proyectados solo a principales de aplicación.
- **Trait `LiveIndex` (`gs-mobility`):**
  - `H3LiveIndex` para despliegues de un nodo (`GS_STATE_BACKEND=memory`).
  - `ValkeyGeoIndex` en `gs-ephemeral` (`GEOADD`, `HSET`, `ZADD`, `pool_upsert.lua`) para despliegues multi-nodo.
  - Se actualiza fuera del camino de `PublishAck` mediante un canal acotado con coalescencia por miembro; indexa la posición estimada (`LocationEstimate`) cuando existe.
- **Herramientas en `gs-mobility`:** estimación de ETA por niveles (`PROVIDER → LEARNED → ESTIMATED`), algoritmo Hungarian (Kuhn–Munkres) y flujo de coste mínimo deterministas.

## Consecuencias

+ Reutiliza el 100 % de las garantías de seguridad, revocación y consentimiento de las sesiones.
+ Un dispositivo puede publicar la misma muestra (misma `sequence`) en su `POOL` (`BUSY`) y en la sesión activa de su viaje simultáneamente.

## Seguridad, privacidad y operación

Se incorpora la amenaza T-25 en `10_SECURITY_PRIVACY.md`: `nearby` y `rings` exigen principal de aplicación (`pools:query`), mientras `density` (`pools:density`) devuelve marcadores anónimos sin IDs, cuantizados al centroide H3 con desplazamiento aleatorio y retraso `pool_density_delay_s`.

## Plan de migración

El contrato entra en E0 (T-E0-06) y la implementación en E4 (pools e índice vivo) y E5 (ETA y asignación).

## Evidencia y referencias

- Decisión D-09 en `docs/proposals/GeoStream_Addendum_A02_Motor_Ubicacion_Movilidad.md` §3 y §9.
- `docs/23_MOBILITY.md`, `docs/05_DOMAIN_MODEL.md`.
