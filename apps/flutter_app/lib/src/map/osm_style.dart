/// Estilo MapLibre con teselas ráster de OpenStreetMap.
///
/// `docs/13_MAPS_PROVIDERS.md` §2: los servidores comunitarios de OSM no son
/// aptos para uso intensivo en producción y la atribución ODbL es obligatoria.
/// Por eso la URL de teselas es configurable en compilación
/// (`--dart-define=GS_TILE_URL=…`) y la atribución se muestra siempre.
library;

import 'dart:convert';

/// Plantilla de teselas por defecto: servidor comunitario de OSM (solo pruebas).
const String osmCommunityTiles =
    'https://tile.openstreetmap.org/{z}/{x}/{y}.png';

/// Plantilla efectiva, configurable con `--dart-define=GS_TILE_URL=…`.
const String tileUrlTemplate = String.fromEnvironment(
  'GS_TILE_URL',
  defaultValue: osmCommunityTiles,
);

/// Whether se usan los servidores comunitarios (`allowCommunityServers`).
bool get usesCommunityServers => tileUrlTemplate == osmCommunityTiles;

/// Atribución obligatoria de los datos.
const String osmAttribution = '© OpenStreetMap contributors';

/// Documento de estilo MapLibre con una sola capa ráster.
String osmRasterStyle({String tiles = tileUrlTemplate}) => jsonEncode({
  'version': 8,
  'sources': {
    'osm': {
      'type': 'raster',
      'tiles': [tiles],
      'tileSize': 256,
      'maxzoom': 19,
      'attribution': osmAttribution,
    },
  },
  'layers': [
    {'id': 'osm', 'type': 'raster', 'source': 'osm'},
  ],
});
