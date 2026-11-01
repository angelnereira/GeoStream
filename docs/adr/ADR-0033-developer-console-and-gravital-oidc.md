# ADR-0033: Consola de desarrolladores con inicio de sesión Gravital (OIDC), registro público y cuotas

- **Estado:** Aceptado
- **Fecha:** 2026-10-31
- **Responsable:** propietario del proyecto
- **Requisitos afectados:** FR-ADM-004, FR-CON-001 a FR-CON-008
- **Sustituye a:** — (amplía ADR-0006: añade un tercer tipo de principal, el desarrollador de consola)
- **Decisión de origen:** DR-20261031-05

## Contexto

GeoStream se ofrece como plataforma a terceros, en el modelo de Google Maps Platform: un desarrollador entra en una
consola web, crea un proyecto, obtiene una API key y consume el servicio con cuotas. Hasta ahora tenants, aplicaciones
y API keys solo se crean con la CLI `geostream admin` (FR-ADM-001), que exige acceso al servidor.

El propietario eligió (DR-20261031-05): inicio de sesión con la cuenta de Gravital por OIDC, registro público con
cuotas gratuitas, cobro en una fase posterior y portal en Flutter Web.

Hechos que condicionan el diseño:

- `06` §2: GeoStream no tiene usuarios finales y la identidad de un dispositivo sale de un token emitido por GeoStream.
  Nada de esto cambia: el desarrollador de consola es un principal **nuevo y separado**, que nunca publica ni lee
  ubicaciones.
- La identidad de Gravital firma hoy con secretos simétricos y no es un proveedor OIDC. Federar con un secreto
  compartido permitiría a GeoStream acuñar sesiones de Gravital: inaceptable (`10` §5.6).

## Opciones consideradas

1. **Consola en Flutter Web + API de consola en el binario `geostream` + Gravital como proveedor OIDC.** Reutiliza el
   stack, la lista aprobada de dependencias y el modelo de tenant/aplicación/API key existente.
2. **Consola con cuentas propias (email + OTP/passkey).** Más rápida de arrancar, pero crea un segundo sistema de
   cuentas que después habría que fusionar con Gravital. Descartada por el propietario.
3. **Portal separado en React/Next.js con su propio backend.** Duplica autorización y exige ADR de dependencias.
   Descartada por el propietario.

## Decisión

### 1. Piezas

| Pieza | Dónde | Qué hace |
|---|---|---|
| Portal | `apps/console` (Flutter Web, compilado a estático) | Interfaz de la consola. Servicio propio en Railway, `console.geostream.gravitalcloud.com` |
| API de consola | rol `console` del binario `geostream`, rutas `/v1/console/*` | Organizaciones, proyectos, API keys, cuotas y uso. Misma imagen, mismos crates |
| Proveedor de identidad | `identity` de Gravital (repositorio Plenty-Pay) | OIDC: inicio de sesión, emisión de tokens firmados con clave asimétrica |

### 2. Inicio de sesión (OIDC, Authorization Code + PKCE)

- El portal es un **cliente público** OIDC (`GS_CONSOLE_OIDC_CLIENT_ID`) que usa *Authorization Code* con PKCE
  (RFC 7636, `S256`). Sin secreto de cliente en el navegador; sin flujo implícito.
- La API de consola acepta solo un *access token* de Gravital en `Authorization: Bearer`, y lo verifica contra el JWKS
  publicado en el descubrimiento de `GS_CONSOLE_OIDC_ISSUER`: firma (EdDSA o ES256; `none` y HS* rechazados), `iss`
  exacto, `aud` que contiene `GS_CONSOLE_OIDC_AUDIENCE`, `exp`/`nbf` con la holgura de `GS_JWT_LEEWAY_S`. El JWKS se
  cachea `GS_CONSOLE_OIDC_JWKS_CACHE_TTL_S` y se recarga ante un `kid` desconocido, como mucho una vez por
  `GS_CONSOLE_OIDC_JWKS_MIN_REFRESH_S`.
- La identidad del desarrollador es el par `(iss, sub)` del token. El email, si viene, es solo un dato de contacto
  verificado por Gravital; nunca la identidad.
- Sin cookies de sesión en la API (no hay CSRF que defender): el portal guarda el token en memoria, nunca en
  `localStorage`.

### 3. Modelo de datos

| Concepto de consola | Entidad existente | Nueva |
|---|---|---|
| Cuenta de desarrollador | — | `console_accounts` (`acc_…`): `oidc_issuer`, `oidc_subject`, email de contacto, estado |
| Organización | `tenants` | `tenant_memberships`: cuenta → tenant con rol `OWNER`, `ADMIN` o `VIEWER` |
| Proyecto | `applications` | — |
| Credencial | `api_keys` (hash, se muestra una sola vez, FR-AUTH-001) | — |
| Cuotas y uso | política del tenant (`17` §4) | contadores de uso por aplicación y periodo |

La consola **no** ve ni exporta ubicaciones, subjects, dispositivos ni sesiones de los usuarios finales del integrador:
solo metadatos de proyecto, keys y uso agregado.

### 4. Registro público y cuotas

- La primera vez que una cuenta Gravital entra, se crea su `console_account`. Crear una organización está limitado por
  `GS_CONSOLE_MAX_ORGS_PER_ACCOUNT` y por un límite de tasa por cuenta e IP (`GS_CONSOLE_SIGNUP_RPM_PER_IP`).
- Cada organización nueva nace con la política por defecto de `17` §4, en la que **todas las capacidades opcionales de
  la pista E siguen apagadas**, más las cuotas gratuitas: `quota_monthly_location_samples`,
  `quota_monthly_rest_requests`, `quota_concurrent_stream_connections` y `max_applications_per_tenant`.
- Superar una cuota mensual devuelve `429 QUOTA_EXCEEDED` (código nuevo del catálogo, `09` §6) hasta el siguiente
  periodo. Las cuotas se cuentan por aplicación en el almacén efímero y se consolidan en PostgreSQL.
- No hay cobro en esta versión. La medición de uso deja preparado el cobro, que exigirá su propio ADR.

## Consecuencias

\+ GeoStream pasa a ser autoservicio sin acceso al servidor; la CLI sigue existiendo para operación.
\+ Reutiliza tenant/aplicación/API key: el camino de datos no cambia.
− Depende de que Gravital publique OIDC con claves asimétricas (T-F15-02, en otro repositorio). Hasta entonces la
consola no puede abrirse al público.
− Añade un principal nuevo y una superficie expuesta a Internet con registro abierto: más abuso posible.

Documentos y contratos que cambian:

- En este paso (T-F15-01, sin código): `03` (FR-ADM-004 y FR-CON-*), `10` (T-32 a T-37), `17` (§2.12 y cuotas de §4),
  `21` (fase F15), `TRACEABILITY.md`.
- En T-F15-03 (único PR que toca `contracts/`): OpenAPI `/v1/console/*`, esquemas, prefijo `acc_`, código
  `QUOTA_EXCEEDED`, scopes de consola; `06` (principal de consola y matriz de autorización), `07`, `09`, `11`
  (migración de `console_accounts`, `tenant_memberships` y uso).

## Seguridad, privacidad y operación

- Amenazas nuevas T-32 a T-37 en `10` §4: suplantación del proveedor OIDC, robo del token en el navegador, abuso del
  registro, escalada entre organizaciones, fuga de API keys desde la consola y agotamiento por cuotas.
- La consola nunca muestra una API key después de crearla; solo su prefijo. Rotar o revocar una key queda auditado
  (`10` §8.3) con la cuenta de consola como actor.
- Cabeceras del portal: CSP estricta sin `unsafe-inline` para scripts, `frame-ancestors 'none'`, HSTS.
- Coste: un servicio estático más en Railway y el rol `console` en la misma imagen.

## Plan de migración

Aditivo. Las organizaciones creadas por CLI siguen funcionando; un operador puede vincularlas a una cuenta de consola
con `geostream admin` (T-F15-05). Sin cambios para integradores ni SDK.

## Evidencia y referencias

- OpenID Connect Core 1.0 y Discovery 1.0; RFC 7636 (PKCE); RFC 8725 (buenas prácticas JWT); RFC 9700 (seguridad de
  OAuth 2.0), consultados el 2026-10-31.
- Servicio `identity` de `gravital-production` en Railway, inspeccionado el 2026-10-31: variables
  `ACCESS_TOKEN_SECRET`/`REFRESH_TOKEN_SECRET`, sin dominio público.
