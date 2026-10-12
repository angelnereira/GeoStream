# ADR-0008: Secuencia monotónica por instalación con sequence floor del servidor

- **Estado:** Aceptado (baseline v0.2.0)
- **Fecha:** 2026-10-11
- **Responsable:** propietario del proyecto
- **Requisitos afectados:** FR-LOC-003, FR-LOC-008, FR-SDK-005

## Contexto

Hay que deduplicar y ordenar muestras ante reconexiones y reenvíos sin depender de relojes. En 0.1.0 la secuencia podía reiniciarse (reinstalación, pérdida de estado) y bloquear todas las publicaciones.

## Opciones consideradas

1. Orden por `captured_at`.
2. `event_id` aleatorio con conjunto de deduplicación.
3. Secuencia monotónica por instalación persistida por bloques en el SDK, CAS por (dispositivo, sesión) en el servidor, y `sequence_floor` en el `ServerHello`.

## Decisión

Opción 3. Una instalación nueva es un dispositivo nuevo (estado limpio). Si el SDK pierde el contador pero conserva el `installation_id`, el servidor informa la mayor secuencia conocida y el SDK salta por encima. El tiempo solo se usa para descartar muestras demasiado antiguas, nunca para ordenar.

## Consecuencias

+ Correcto con relojes desfasados; deduplicación O(1); sin bloqueo tras pérdida de estado.
- Huecos en la secuencia (permitidos); el almacén efímero guarda contadores por par.

## Seguridad, privacidad y operación

La secuencia no es secreta ni identifica a la persona.

## Referencias

`09_VALIDATION_ERRORS.md` §4, `12_FLUTTER_SDK.md` §9
