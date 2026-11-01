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
| `PORTAL_API_UPSTREAM` | `https://geostream.gravitalcloud.com` | API cuyo `/health/ready` expone `/status/ready` |

Rutas: `/` (aplicación), `/status/ready` (proxy del estado), `/docs/…` (guías y referencia de la API con Redoc
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
