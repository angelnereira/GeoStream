# Portal público de GeoStream

Flutter Web (ADR-0034): landing, estado en vivo y enlaces a la documentación y los contratos publicados. La consola
con inicio de sesión (ADR-0033) y el mapa en vivo se añaden aquí en T-F15-12 a T-F15-14.

No es un crate: queda fuera del workspace de Cargo y de la imagen del servidor.

## Qué se publica

`docs-manifest.txt` lista, de forma explícita, los contratos (`openapi`, `proto`) y las guías (`doc`) que genera la
imagen. `cargo run -p geostream-tools -- portal-docs` falla si aparece un documento fuera de la lista permitida (nunca
`10`, `16`, `17` ni `docs/decisions/`) o si falta algún contrato (FR-PRT-001, FR-PRT-003).

## Desarrollo

```sh
cd apps/portal
flutter pub get
flutter analyze && flutter test
flutter run -d chrome        # sin proxy: el estado se muestra como «Sin datos»
```

## Imagen

Se construye desde la **raíz** del repositorio (usa `Dockerfile.dockerignore` propio):

```sh
docker build -f apps/portal/Dockerfile -t geostream-portal .
docker run --rm -p 8080:8080 geostream-portal
```

| Variable | Default | Efecto |
|---|---|---|
| `PORT` | `8080` | Puerto de nginx (Railway lo define) |
| `PORTAL_API_UPSTREAM` | `https://geostream.gravitalcloud.com` | API a la que apuntan `/status/ready` y el proxy de solo lectura `/api/…` |
| `PORTAL_TILES_UPSTREAM` | `http://tiles.railway.internal:8080` | Servicio `tiles` (mapa propio) detrás de `/tiles/…`; el portal arranca aunque aún no exista |

Rutas: `/` (aplicación), `/status/ready` (proxy del estado), `/api/v1/sessions…` (proxy de **solo lectura**, ADR-0035),
`/tiles/…` (mapa propio con caché), `/map/style.json`, `/vendor/maplibre-gl/…` (MapLibre alojado en el portal), `/docs/…` (guías y referencia de la API con Redoc
alojado en el propio portal, sin CDN) y `/contracts/…` (OpenAPI y `.proto`).

## Despliegue en Railway

Servicio nuevo en el proyecto `geostream`, desde este repositorio y la rama `main`:
`Dockerfile` en `apps/portal/Dockerfile`, **sin** directorio raíz (el contexto es la raíz del repositorio), rutas
vigiladas `apps/portal/**`, `contracts/**` y `docs/**`, healthcheck `/`. Dominio previsto:
`console.geostream.gravitalcloud.com` (registro `CNAME` que muestra Railway).

## Estado conocido

- El estado en vivo exige el cuerpo `ReadinessStatus` del contrato (`{"status","checks"}`). Mientras la API
  desplegada devuelva el cuerpo anterior (`{"ready","dependencies"}`, corregido en T-F6-03), el portal muestra
  «Sin datos»: no interpreta lo que no cumple el contrato.
- El logo «API docs by Redocly» del pie de la referencia viene de un CDN y la CSP lo bloquea; no afecta al contenido.

## Acceso con API key y mapa en vivo

La pantalla «Ver mis sesiones en el mapa» pide una API key, la guarda solo en memoria (se pierde al recargar) y la
envía únicamente por `/api/…`. nginx solo reenvía `GET`/`HEAD` de sesiones, una sesión, sus miembros y su snapshot;
todo lo demás responde 404 (ADR-0035). El mapa consulta el snapshot cada 5 s. Usa una key con solo `sessions:read` y
`locations:read`: el portal no puede comprobar los permisos de la key, así que no te protege de pegar una de escritura.

Dos trampas ya resueltas y que conviene recordar al tocar la imagen:

- MapLibre carga un worker, y el worker hereda la CSP de **su propio script**: los archivos de `/vendor/` llevan
  `connect-src 'self'` para poder pedir teselas. Con `default-src 'none'` el mapa queda en blanco sin errores visibles.
- En nginx, `rewrite … break` corta las directivas posteriores: el `set` del destino va antes del `rewrite`.
