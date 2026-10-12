# ADR-0007: Autenticación del stream con tickets de un solo uso y renovación dentro de la conexión

- **Estado:** Aceptado (baseline v0.2.0)
- **Fecha:** 2026-10-11
- **Responsable:** propietario del proyecto
- **Requisitos afectados:** FR-AUTH-006, FR-AUTH-007

## Contexto

Los navegadores no permiten cabeceras personalizadas en WebSocket; poner tokens de larga duración en la URL los expone en logs. Las conexiones duran más que los tokens.

## Opciones consideradas

1. Token en la query.
2. Ticket opaco de un solo uso (30 s) obtenido por REST y canjeado en el upgrade; `AuthExpiring`/`AuthRefresh` dentro de la conexión.
3. Autenticación en el primer mensaje.

## Decisión

Opción 2. El ticket se canjea con get-and-delete antes del upgrade (rechazo barato de conexiones no autenticadas). El parámetro se redacta en logs. Los dispositivos renuevan con `AuthRefresh` del mismo `did`; las aplicaciones reconectan antes de `GS_STREAM_APP_MAX_CONNECTION_AGE_S`.

## Consecuencias

+ Sin secretos reutilizables en URLs; funciona en todas las plataformas; la revocación se respeta durante conexiones largas.
- Una petición REST extra por conexión.

## Seguridad, privacidad y operación

Ticket ligado al principal y a sus scopes; TTL configurable 5–120 s.

## Referencias

`06_IDENTITY_AUTH.md` §5, `08_STREAM_PROTOCOL.md` §3, §6.6
