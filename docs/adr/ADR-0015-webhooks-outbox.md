# ADR-0015: Webhooks con outbox transaccional, firma HMAC y entrega at-least-once

- **Estado:** Aceptado (baseline v0.2.0)
- **Fecha:** 2026-10-11
- **Responsable:** propietario del proyecto
- **Requisitos afectados:** FR-WHK-001..005

## Contexto

Los sistemas terceros necesitan enterarse de cambios (sesiones, miembros, compartición, geocercas) sin sondear, y los eventos no deben perderse si el proceso cae.

## Opciones consideradas

1. Envío directo desde el handler.
2. Outbox en la misma transacción + worker con reintentos + firma HMAC-SHA256 con timestamp.
3. Broker de mensajería externo.

## Decisión

Opción 2. Una fila por endpoint suscrito en `webhook_deliveries` dentro de la transacción del cambio; worker con `SKIP LOCKED`, calendario de reintentos de `07` §10.4, deshabilitación tras fallos prolongados, protección SSRF y sin coordenadas en los payloads.

## Consecuencias

+ Sin pérdida de eventos; sin infraestructura adicional; verificación sencilla para receptores.
- Entrega at-least-once sin orden: los receptores deben deduplicar.

## Seguridad, privacidad y operación

Secreto de webhook cifrado en reposo; destinos privados bloqueados.

## Referencias

`07_REST_API.md` §10
