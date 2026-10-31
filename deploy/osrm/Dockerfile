# Motor de rutas OSRM sobre OpenStreetMap (Panamá) para dibujar la ruta del repartidor al cliente
# (decisión del dueño 2026-10-31). Los datos se descargan y preprocesan al construir la imagen.
# Licencia de datos: ODbL (© OpenStreetMap contributors).
FROM debian:bookworm-slim AS data
ARG OSM_PBF_URL=https://download.geofabrik.de/central-america/panama-latest.osm.pbf
RUN apt-get update && apt-get install -y --no-install-recommends curl ca-certificates \
 && rm -rf /var/lib/apt/lists/*
RUN mkdir -p /data && curl -fsSL -o /data/region.osm.pbf "${OSM_PBF_URL}"

FROM ghcr.io/project-osrm/osrm-backend:v5.27.1 AS build
COPY --from=data /data /data
RUN osrm-extract -p /opt/car.lua /data/region.osm.pbf \
 && osrm-partition /data/region.osrm \
 && osrm-customize /data/region.osrm \
 && rm /data/region.osm.pbf

FROM ghcr.io/project-osrm/osrm-backend:v5.27.1
COPY --from=build /data /data
ENV PORT=5000
EXPOSE 5000
CMD ["sh", "-c", "exec osrm-routed --algorithm mld --port ${PORT} --max-table-size 100 /data/region.osrm"]
