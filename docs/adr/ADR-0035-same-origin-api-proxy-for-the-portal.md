# ADR-0035: El portal llama a la API por un proxy del mismo origen, sin CORS

- **Estado:** Aceptado
- **Fecha:** 2026-11-01
- **Responsable:** propietario del proyecto (decisión de alcance: «lo que sea viable», DR-20261101-01)
- **Requisitos afectados:** FR-PRT-004, FR-INT-003
- **Sustituye a:** la cláusula de CORS de ADR-0034 §4 (el resto de ADR-0034 sigue vigente)

## Contexto

ADR-0034 §4 decía que el acceso con API key desde el navegador «requiere CORS por aplicación (T-F15-11)». Al diseñarlo
aparece un problema que `07` §12 no resuelve:

- Un *preflight* (`OPTIONS`) no lleva `Authorization`: el servidor no sabe a qué aplicación pertenece, así que no
  puede aplicar `application.allowed_origins` y habría que aceptar la **unión** de los orígenes de todas las
  aplicaciones, que da a un origen permitido para X la lectura de respuestas de la aplicación Y.
- La autenticación se resuelve dentro de cada handler, no en una capa: una capa CORS por aplicación obliga a volver a
  autenticar la credencial o a tocar todos los handlers. Es un cambio de seguridad sensible y no está especificado
  (`20` §6, condición 2).

## Opciones consideradas

1. **CORS por aplicación en el servidor** (T-F15-11 original). Necesario algún día para clientes web de terceros, pero
   exige una decisión de seguridad propia (unión en el preflight o nueva autenticación en capa).
2. **Proxy del mismo origen en el portal.** nginx publica `/api/…` con solo las rutas de lectura que usa el portal y
   las reenvía a la API. El navegador solo habla con su propio origen: no hay CORS, ni preflight, ni unión de orígenes.
3. **Una lista global de orígenes en la configuración del servidor.** Sencillo, pero no es por aplicación y contradice `07` §12.

## Decisión

Opción 2 para el portal. La Opción 1 sigue siendo FR-INT-003 (SHOULD, F8) para clientes web de terceros, con su propia
decisión de seguridad cuando se aborde.

- El portal reenvía **solo** las rutas que necesita para ver sesiones y mapa, y solo de lectura:
  `GET /v1/sessions`, `GET /v1/sessions/{id}`, `GET /v1/sessions/{id}/snapshot`, `POST /v1/stream/tickets` y el
  `GET /v1/stream` (WebSocket). Cualquier otra ruta o método devuelve 404 en el borde, de modo que el portal no puede
  escribir aunque la key lo permita (defensa en profundidad de `10` T-38).
- nginx no registra cabeceras ni cuerpos (`Authorization` nunca llega a un log), añade `Cache-Control: no-store` y
  limita la tasa por IP.
- El navegador conserva la key solo en memoria (ADR-0034 §4) y la envía a su propio origen; `connect-src 'self'`
  se mantiene.
- El proxy es un destino fijo (`PORTAL_API_UPSTREAM`): no reenvía a hosts que elija el cliente (sin SSRF).

## Consecuencias

\+ Sin cambios en el servidor ni en contratos; el portal no depende de CORS.
\+ La superficie que ve el navegador queda reducida a unas pocas rutas de lectura.
− La key pasa por la infraestructura del portal (misma organización; no se registra).
− Un tercero que quiera llamar a la API desde su propio sitio web sigue necesitando FR-INT-003.

Cambia: `21` (T-F15-11 deja de ser prerrequisito del portal; T-F15-12 incluye el proxy), `03` (FR-PRT-004), `10`
(T-38), `docs/adr/README.md`. Ningún contrato público.

## Seguridad, privacidad y operación

Reduce T-38 (la key no se usa desde un origen cruzado y el borde impide escribir). Añade el proxy como superficie
propia: sus pruebas cubren método, ruta y ausencia de registro de `Authorization` (T-F15-12).

## Plan de migración

Aditivo. Nada cambia para integradores.

## Evidencia y referencias

`07` §12; ADR-0034; revisión de `crates/gs-api/src/security.rs` y de los handlers (2026-11-01).

## Erratas de implementación (2026-11-01, T-F15-12)

Al implementarlo aparecieron tres precisiones que no cambian la decisión:

1. **La API no ofrece forma de consultar los scopes de una key** (no hay introspección en el contrato), así que el
   portal **no puede rechazar** una key con permisos de escritura, como decía ADR-0034 §4. La garantía de solo lectura
   la da el borde: nginx solo reenvía `GET`/`HEAD` de `/v1/sessions`, `/v1/sessions/{id}`, `/v1/sessions/{id}/members`
   y `/v1/sessions/{id}/snapshot`; cualquier otra ruta o método devuelve 404. La interfaz pide una key de solo lectura,
   pero no puede comprobarlo. Una introspección (`GET /v1/credentials/me`) sería un cambio de contrato para T-F15-03.
2. **El mapa se actualiza consultando el snapshot cada 5 s**, no por el stream. En producción `gs.v1.json` está apagado
   (`17`) y el binario exige código Dart generado de los `.proto`, que queda para una tarea posterior. Por eso la ruta
   `POST /v1/stream/tickets` y `GET /v1/stream` no están en la lista de rutas reenviadas.
3. **Un worker hereda la CSP de la respuesta de su propio script.** El worker de MapLibre pide las teselas, así que los
   archivos de `/vendor/` se sirven con `connect-src 'self'`; con `default-src 'none'` el mapa queda en blanco sin
   ningún error visible en la página.
