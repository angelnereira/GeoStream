# ADR-0010: Backends del almacén efímero y del bus: memoria y Redis/Valkey

- **Estado:** Aceptado (baseline v0.2.0)
- **Fecha:** 2026-10-11
- **Responsable:** propietario del proyecto
- **Requisitos afectados:** FR-STR-009, FR-PRS-003, NFR-RES-002

## Contexto

En desarrollo e instalaciones pequeñas no conviene exigir Redis; en multi-nodo hace falta estado compartido y distribución entre instancias.

## Opciones consideradas

1. Siempre Redis.
2. Traits con dos implementaciones: memoria (una instancia) y Redis/Valkey (multi-nodo), probadas con la misma suite.
3. Kafka/NATS para el bus.

## Decisión

Opción 2. Redis/Valkey ≥ 7.2 con Pub/Sub clásico, scripts Lua para operaciones atómicas, sin persistencia y `noeviction`. Redis Streams, cluster o NATS solo con evidencia y ADR. Valkey se recomienda por su licencia BSD; verificar la licencia vigente de Redis al elegir.

## Consecuencias

+ Desarrollo sin dependencias; semántica idéntica verificada; operación simple.
- `memory` impone una sola instancia (documentado y advertido en `prod`).

## Seguridad, privacidad y operación

Caída del almacén efímero → degradación explícita (`04` §7.6), nunca fan-out silenciosamente roto.

## Referencias

`11_DATA_STORAGE.md` §3, `16_OPERATIONS.md` §2.2
