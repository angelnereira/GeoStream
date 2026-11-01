# ADR-0034: Portal público (landing, documentación, estado y demo) y mapa propio

- **Estado:** Aceptado
- **Fecha:** 2026-11-01
- **Responsable:** propietario del proyecto
- **Requisitos afectados:** FR-PRT-001 a FR-PRT-006, FR-MAP-007, FR-INT-003
- **Sustituye a:** — (amplía ADR-0033: la consola sigue siendo la parte autenticada del mismo portal)
- **Decisión de origen:** DR-20261101-01

## Contexto

GeoStream se ofrece como plataforma (ADR-0033). Falta la cara pública: una landing que explique el producto y publique
los contratos, que muestre si el servicio está disponible, que deje probarlo sin cuenta y que dé entrada a los
integradores. Además, los mapas del portal no pueden depender de los servidores comunitarios de OpenStreetMap
(`13` §2).

## Opciones consideradas

1. **Un solo portal Flutter Web** (landing, documentación publicada, estado, demo y, después, consola) servido como
   sitio estático detrás de un nginx que también hace de proxy del estado. Reutiliza el stack; una sola
   superficie pública.
2. **Landing aparte en HTML y consola en Flutter.** Mejor para buscadores, pero dos sitios, dos despliegues y estilos
   duplicados.
3. **Servir la landing desde el binario Rust.** Mezcla una interfaz web con la API y su despliegue.

## Decisión

### 1. Piezas

| Pieza | Dónde | Qué hace |
|---|---|---|
| Portal | `apps/portal` (Flutter Web), imagen con nginx, servicio `portal` en Railway | Landing, enlaces a la documentación, estado, demo; la consola de ADR-0033 se añade aquí |
| Documentación publicada | HTML estático generado en la construcción de la imagen | OpenAPI renderizado (Redocly), `.proto` y guías en HTML |
| Estado | `GET /status/ready` del propio portal | nginx hace proxy de `GET /health/ready` de la API: mismo origen, sin CORS |
| Mapa propio | servicio `tiles` (T-F15-10) | Teselas vectoriales derivadas del extracto de Panamá de Geofabrik, con atribución ODbL |

### 2. Documentación publicada

Se publican **solo** los contratos y las guías para integradores: `contracts/openapi/geostream.v1.yaml`,
`contracts/proto/geostream/v1/*.proto` y los documentos `07`, `08`, `09`, `12`, `13` y `15`. **No** se publican `10`
(modelo de amenazas), `16` (operaciones), `17` (configuración interna) ni `docs/decisions/`. La lista vive en un
manifiesto del repositorio (`apps/portal/docs-manifest.txt`) y una prueba falla si contiene un documento no permitido.
Las afirmaciones de rendimiento llevan su etiqueta de `19` §9: objetivo, no medido.

### 3. Estado

El portal muestra el resultado de `/health/ready` (`ReadinessStatus`: `status` y `checks`), con la hora de la
consulta. Si el proxy no responde, muestra «sin datos», **nunca** «disponible». No hay porcentaje de disponibilidad
hasta que exista medición (`20` §9.3).

### 4. Acceso de integradores mientras no hay OIDC

- El portal acepta una **API key de aplicación de solo lectura** (`sessions:read`, `locations:read`) y la guarda
  **solo en memoria**: se pierde al recargar la página. Nunca en `localStorage`, `sessionStorage` ni cookies.
- La key se envía únicamente en `Authorization: Bearer` a `GS_PUBLIC_BASE_URL`.
- Requiere CORS por aplicación (T-F15-11): solo los orígenes de `application.allowed_origins` reciben cabeceras
  CORS, `Allow-Credentials: false` (`07` §12).
- El portal no pide ni acepta keys con scopes de escritura: si la key tiene alguno de `tokens:mint`, `sessions:write`,
  `members:write` o `subjects:manage`, la rechaza con un mensaje que explica por qué.
- Cuando exista OIDC (T-F15-02) este acceso se mantiene como alternativa para integradores y el inicio de sesión de
  Gravital pasa a ser el camino principal.

### 5. Demo

Una organización de demostración con repartidores **simulados** por un generador del lado del servidor, solo lectura,
sin cuenta. Los datos están marcados como simulados en la interfaz (`01` P-6: nunca presentar datos estimados como
medidos). La demo no usa ubicaciones de personas reales y tiene límites propios de conexiones y de tasa.

### 6. Mapa propio

- Teselas vectoriales servidas por un servicio propio a partir del extracto de OpenStreetMap de Panamá; estilo
  abierto; atribución «© OpenStreetMap contributors» siempre visible (ODbL, `13` §2).
- Es `ProviderId.selfHosted` de ADR-0013: el portal y la app de pruebas lo usan en lugar de `tile.openstreetmap.org`.
- La construcción de teselas es una herramienta de despliegue, no una dependencia del código Rust ni Dart.
- Qué tamaño de región y qué pie de memoria necesita se **mide** al construirlo (T-F15-10); no se afirma antes.

## Consecuencias

\+ Una sola superficie pública, un solo despliegue de interfaz, sin dependencias nuevas de Dart.
\+ El estado y la documentación no necesitan cambios en el servidor.
− El portal Flutter Web se indexa peor que un HTML: la documentación publicada sí es HTML estático y se indexa.
− El acceso con API key lleva una credencial al navegador: riesgo aceptado y acotado (T-38).
− El mapa propio añade un servicio con coste de CPU y disco.

Documentos y contratos que cambian: `03` (FR-PRT-*, FR-MAP-007), `10` (T-38 a T-40), `21`, `TRACEABILITY.md`,
`docs/adr/README.md`. Ningún contrato público.

## Seguridad, privacidad y operación

- T-38: key en el navegador. T-39: abuso de la demo. T-40: publicar documentación interna por error.
- Cabeceras del portal: CSP estricta (`default-src 'self'`; `connect-src` solo la API, el mapa propio y el estado),
  `frame-ancestors 'none'`, HSTS, `X-Content-Type-Options: nosniff`.
- Costes: un servicio estático y un servicio de teselas en Railway.

## Plan de migración

Aditivo. Nada cambia para integradores existentes.

## Evidencia y referencias

- `13` §2 (política de los servidores comunitarios de OpenStreetMap, consultada en el documento el 2026-10-11).
- ADR-0013, ADR-0033, DR-20261101-01.
