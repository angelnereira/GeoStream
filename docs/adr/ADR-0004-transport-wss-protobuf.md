# ADR-0004: WebSocket seguro con Protobuf binario y mapeo JSON

- **Estado:** Aceptado (baseline v0.2.0)
- **Fecha:** 2026-10-11
- **Responsable:** propietario del proyecto
- **Requisitos afectados:** FR-STR-001, FR-LOC-009, FR-SDK-014

## Contexto

Se necesita un canal bidireccional soportado por Flutter (móvil y web), navegadores y proxies, con un contrato versionable y eficiente.

## Opciones consideradas

1. WebSocket + Protobuf binario + JSON canónico proto3 como subprotocolos.
2. gRPC streaming.
3. MQTT.
4. Solo JSON.

## Decisión

WSS como transporte predeterminado con subprotocolos `gs.v1.proto` (binario, obligatorio) y `gs.v1.json` (texto, deshabilitable en producción). Protobuf (`contracts/proto`) es la fuente de verdad; el JSON es su mapeo canónico (pbjson en Rust). REST para el plano de control. gRPC queda para comunicación interna si algún día se separan servicios; MQTT es pista F14; WebRTC pista F16. Ingesta HTTP (F8) reutiliza las mismas reglas.

## Consecuencias

+ Compatibilidad amplia, mensajes compactos, depuración legible con JSON, evolución controlada con `buf breaking`.
- Hay que implementar reconexión, backpressure y autorización propias.

## Seguridad, privacidad y operación

Sin `permessage-deflate` en v1. Tickets de un solo uso en la URL (ADR-0007).

## Referencias

`08_STREAM_PROTOCOL.md`
