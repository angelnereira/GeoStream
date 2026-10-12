# ADR-0006: Modelo de identidad: API keys de aplicación y tokens de dispositivo emitidos por GeoStream

- **Estado:** Aceptado (baseline v0.2.0)
- **Fecha:** 2026-10-11
- **Responsable:** propietario del proyecto
- **Requisitos afectados:** FR-AUTH-001..005, FR-AUTH-008, FR-SDK-002

## Contexto

La versión 0.1.0 dejaba abierto cómo se autentican usuarios y dispositivos de las apps integradoras. El SDK no puede contener secretos de servidor y el integrador debe seguir controlando quién es un usuario válido.

## Opciones consideradas

1. El integrador firma JWT con un secreto compartido o una clave registrada (sin llamada a GeoStream).
2. El backend del integrador pide tokens a GeoStream con una API key (`POST /v1/tokens`); GeoStream firma con Ed25519; sin refresh tokens.
3. OAuth2/OIDC completo con GeoStream como servidor de autorización.

## Decisión

Opción 2. Entidades `Subject` (usuario del integrador, `external_subject_id`) por encima de `Device` (instalación). Tokens EdDSA de vida corta con claims fijos (`06` §4.2), `token_epoch` por subject para revocar todos sus tokens, desactivación de dispositivos, scopes de aplicación y de dispositivo. El `CredentialProvider` del SDK llama al backend del integrador.

## Consecuencias

+ GeoStream controla emisión, claims y revocación; el SDK no maneja secretos; el integrador decide la validez del usuario en cada renovación.
- Una llamada server-to-server por emisión de token; disponibilidad de GeoStream necesaria para iniciar sesión en la app.

## Seguridad, privacidad y operación

La API key nunca sale del backend. `locations:read` no se concede por defecto. Alternativa 1 queda como FR-AUTH-009 (F15).

## Referencias

`06_IDENTITY_AUTH.md`
