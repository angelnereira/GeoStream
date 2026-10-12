# ADR-0018: Errores REST con Problem Details y catálogo compartido con el stream

- **Estado:** Aceptado (baseline v0.2.0)
- **Fecha:** 2026-10-11
- **Responsable:** propietario del proyecto
- **Requisitos afectados:** FR-API-001, FR-SDK-009

## Contexto

Los integradores y el SDK necesitan códigos estables y mensajes seguros, iguales en REST y stream.

## Opciones consideradas

1. Formato propio.
2. RFC 9457 Problem Details en REST + enum `ErrorCode` en Protobuf como catálogo único.
3. Solo códigos HTTP.

## Decisión

Opción 2. El nombre REST es el valor Protobuf sin prefijo. Añadir un código es un cambio menor que actualiza proto, OpenAPI, `09` §6 y el SDK en el mismo PR (verificado por CI).

## Consecuencias

+ Un catálogo, verificable automáticamente; errores tipados en el SDK.
- Disciplina para no inventar códigos ad hoc.

## Seguridad, privacidad y operación

Nunca detalles internos en `detail`/`message`.

## Referencias

`09_VALIDATION_ERRORS.md` §6, `07_REST_API.md` §5
