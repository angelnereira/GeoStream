# ADR-0009: Caché de autorización acotada e invalidación por bus para la revocación

- **Estado:** Aceptado (baseline v0.2.0)
- **Fecha:** 2026-10-11
- **Responsable:** propietario del proyecto
- **Requisitos afectados:** NFR-SEC-002, FR-STR-003, FR-STR-008, FR-MEM-004

## Contexto

Los eventos de control deben ser confiables, pero Redis Pub/Sub es fire-and-forget: si una instancia pierde un aviso de revocación, un miembro expulsado podría seguir recibiendo indefinidamente. Consultar PostgreSQL en cada entrega no escala.

## Opciones consideradas

1. Consultar PostgreSQL en cada entrega.
2. Log durable (Redis Streams) para eventos de control.
3. Caché por instancia con TTL corto + versión de membresías por sesión (`mver`) en el almacén efímero + invalidación por bus como acelerador + vaciado total al reconectar el bus.

## Decisión

Opción 3. Plazo de revocación ≤ latencia del bus en condiciones normales y ≤ `GS_AUTHZ_CACHE_TTL_MS` + 1 s en el peor caso. La revalidación por TTL cuesta una lectura de `mver` por sesión, no por suscriptor.

## Consecuencias

+ Garantía acotada y medible sin infraestructura extra; coste bajo.
- Ventana de hasta el TTL en el peor caso (configurable 0,5–30 s, default 5 s).

## Seguridad, privacidad y operación

El principal de cada conexión también se revalida por TTL (tokens revocados, dispositivos desactivados, tenants deshabilitados).

## Referencias

`04_ARCHITECTURE.md` §6.2
