import 'package:http/http.dart' as http;

/// Atribución obligatoria del mapa (ODbL y CC-BY, `13` §2); se muestra siempre sobre el mapa.
const String mapAttribution = '© OpenStreetMap contributors · © OpenMapTiles';

/// Hace absolutas las rutas propias del estilo (`"/tiles/…"`) con [origin].
///
/// Los workers de MapLibre no resuelven URLs relativas («Failed to parse URL»), así que el
/// cliente las completa con su propio origen antes de entregar el estilo (`deploy/README.md`).
String absolutizeStyle(String styleJson, String origin) =>
    styleJson.replaceAll('"/tiles/', '"$origin/tiles/');

/// Descarga el estilo propio del portal (`/map/style.json`) y lo deja listo para MapLibre.
Future<String> loadOwnStyle(http.Client client, Uri base) async {
  final response = await client.get(base.resolve('/map/style.json'));
  if (response.statusCode != 200) {
    throw http.ClientException('estilo no disponible: ${response.statusCode}');
  }
  return absolutizeStyle(response.body, base.origin);
}
