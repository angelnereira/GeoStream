# Portal público de GeoStream (ADR-0034): Flutter Web + documentación publicada + nginx.
# Se construye desde la RAÍZ del repositorio:
#   docker build -f apps/portal/Dockerfile -t geostream-portal .

# --- 1. Documentación publicada: OpenAPI (Redoc) y guías (Markdown → HTML) -----------------
FROM node:20-slim AS docs
WORKDIR /repo
COPY contracts/ contracts/
COPY docs/ docs/
COPY deploy/tiles/style.json deploy/tiles/style.json
COPY apps/portal/docs-manifest.txt apps/portal/docs-manifest.txt
COPY apps/portal/tool/build_docs.mjs /tool/build_docs.mjs
# `marked` convierte las guías y `redoc` (alojado en el propio portal, sin CDN) dibuja el OpenAPI.
# `maplibre-gl` 6.4.1 es la versión contra la que está escrita la interoperabilidad del plugin `maplibre_gl` 0.27.1;
# se aloja entera (el worker se resuelve relativo a su URL) para que la CSP no necesite un CDN.
RUN cd /tool && npm init -y >/dev/null \
 && npm install --no-audit --no-fund marked@12.0.2 redoc@2.5.1 maplibre-gl@6.4.1 \
 && node build_docs.mjs /repo /out \
 && mkdir -p /out/vendor/maplibre-gl /out/map \
 && cp node_modules/maplibre-gl/dist/maplibre-gl.mjs node_modules/maplibre-gl/dist/maplibre-gl-shared.mjs \
       node_modules/maplibre-gl/dist/maplibre-gl-worker.mjs node_modules/maplibre-gl/dist/maplibre-gl.css /out/vendor/maplibre-gl/ \
 && cp /repo/deploy/tiles/style.json /out/map/style.json

# --- 2. Aplicación Flutter Web -----------------------------------------------------------------
FROM debian:bookworm-slim AS flutter
ARG FLUTTER_VERSION=3.47.6
RUN apt-get update && apt-get install -y --no-install-recommends \
      ca-certificates curl git unzip xz-utils zip \
 && rm -rf /var/lib/apt/lists/*
RUN git clone --depth 1 --branch ${FLUTTER_VERSION} https://github.com/flutter/flutter.git /opt/flutter
ENV PATH="/opt/flutter/bin:${PATH}"
RUN flutter config --no-analytics --enable-web && flutter precache --web
WORKDIR /app
COPY apps/portal/pubspec.yaml apps/portal/pubspec.lock ./
RUN flutter pub get
COPY apps/portal/ ./
# `--no-web-resources-cdn`: CanvasKit se sirve desde el propio origen (CSP `script-src 'self'`,
# sin pedir recursos a terceros).
RUN flutter build web --release --no-web-resources-cdn

# --- 3. nginx ---------------------------------------------------------------------------------
FROM nginx:1.27-alpine
# `NGINX_ENTRYPOINT_LOCAL_RESOLVERS` hace que la imagen de nginx exporte `NGINX_LOCAL_RESOLVERS` desde
# /etc/resolv.conf (en Railway, el DNS interno que resuelve `*.railway.internal`).
ENV PORT=8080 \
    PORTAL_API_UPSTREAM=https://geostream.gravitalcloud.com \
    PORTAL_TILES_UPSTREAM=http://tiles.railway.internal:8080 \
    NGINX_ENTRYPOINT_LOCAL_RESOLVERS=1 \
    NGINX_ENVSUBST_FILTER='^(PORT|PORTAL_API_UPSTREAM|PORTAL_TILES_UPSTREAM|NGINX_LOCAL_RESOLVERS)$'
COPY apps/portal/nginx.conf.template /etc/nginx/templates/default.conf.template
COPY apps/portal/security-headers.conf /etc/nginx/security-headers.conf
COPY --from=flutter /app/build/web /usr/share/nginx/html
COPY --from=docs /out/docs /usr/share/nginx/html/docs
COPY --from=docs /out/contracts /usr/share/nginx/html/contracts
COPY --from=docs /out/vendor /usr/share/nginx/html/vendor
COPY --from=docs /out/map /usr/share/nginx/html/map
EXPOSE 8080
