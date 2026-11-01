# Servicio de mapa propio (ADR-0034 §6, FR-MAP-007): teselas vectoriales derivadas de un extracto de
# OpenStreetMap (Panamá, Geofabrik), con el esquema OpenMapTiles, servidas por Martin.
# Las teselas se construyen al construir la imagen, igual que el servicio `osrm`.
#
# Licencias: datos © OpenStreetMap contributors (ODbL); esquema © OpenMapTiles (CC-BY 4.0); Natural Earth
# (dominio público); polígonos de agua de OSM (ODbL); fuente Noto Sans (SIL OFL). La atribución va en el estilo.
#
# Las imágenes se fijan por digest: las etiquetas del proyecto no son estables.

# --- 1. Teselas con Planetiler ---------------------------------------------------------------
FROM ghcr.io/onthegomap/planetiler@sha256:179c83912057f57b266fb46676ae11b96deeba04edf2e25af57d904991b57646 AS build
# Dos pasos a propósito. La descarga (Natural Earth 434 MB, polígonos de agua 932 MB, lagos 80 MB y el extracto)
# depende de servidores de terceros que a veces dan ~200 kB/s (medido el 2026-11-01: más de 25 min solo en descargar).
# Al ser una capa propia, Docker/Railway la reutilizan mientras no cambie esta instrucción, y reconstruir las
# teselas no vuelve a descargar nada.
RUN ["java", "-Xmx3g", "-cp", "@/app/jib-classpath-file", "com.onthegomap.planetiler.Main", \
     "--only-download", "--download", "--area=panama"]
# Sin `--nodemap-type=array`: ese índice reserva memoria para el mayor ID de nodo de OSM y Planetiler lo estima en
# 112 GB para Panamá, así que se niega a empezar (probado: «Insufficient resources for read phase»). El valor por
# defecto, `sparsearray`, es el que recomienda la propia herramienta. Nunca se pasa `--force`: ocultaría justo
# esta clase de fallo y la construcción podría agotar la memoria del constructor.
RUN ["java", "-Xmx3g", "-cp", "@/app/jib-classpath-file", "com.onthegomap.planetiler.Main", \
     "--download", "--area=panama", "--output=/tmp/panama.mbtiles"]

# --- 2. Fuentes para las etiquetas -------------------------------------------------------------
FROM debian:bookworm-slim AS fonts
RUN apt-get update && apt-get install -y --no-install-recommends fonts-noto-core \
 && rm -rf /var/lib/apt/lists/* \
 && mkdir -p /fonts \
 && cp /usr/share/fonts/truetype/noto/NotoSans-Regular.ttf \
       /usr/share/fonts/truetype/noto/NotoSans-Bold.ttf \
       /usr/share/fonts/truetype/noto/NotoSans-Italic.ttf /fonts/

# --- 3. Servidor de teselas ----------------------------------------------------------------------
FROM ghcr.io/maplibre/martin@sha256:59902019bf9038926ff0c71174237d6852e64c457830a6349abe7090be8818ca
COPY --from=build /tmp/panama.mbtiles /data/panama.mbtiles
COPY --from=fonts /fonts /fonts
# Sin privilegios: los archivos son legibles por cualquiera y el puerto es > 1024.
USER 65532:65532
ENV PORT=8080
EXPOSE 8080
# Sirve /panama (TileJSON), /panama/{z}/{x}/{y} y /font/{fontstack}/{range} (glifos generados de /fonts).
ENTRYPOINT ["/bin/sh", "-c", "exec /usr/local/bin/martin --listen-addresses 0.0.0.0:${PORT} --font /fonts /data/panama.mbtiles"]
