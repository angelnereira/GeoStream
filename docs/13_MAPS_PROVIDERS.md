# 13 — Mapas y proveedores

**ID:** GS-MAP-001 · **Versión:** 0.2.0 · **Fase:** F9 · **Decisión:** ADR-0013

## 1. Principio
La ubicación no es el mapa. GeoStream distribuye coordenadas; mostrarlas, geocodificarlas o calcular rutas son responsabilidades separadas, con licencias, costos y cuotas distintos. El núcleo (`geostream`) funciona sin ningún proveedor (FR-SDK-016, NFR-RES-001).

Una "pila de mapa" tiene piezas independientes:

| Pieza | Qué es | Ejemplos |
|---|---|---|
| Renderer | SDK que dibuja el mapa en la app | MapLibre (`maplibre_gl`), Google Maps (`google_maps_flutter`) |
| Datos/teselas y estilo | Las teselas vectoriales o ráster y su estilo | Teselas propias (OpenMapTiles, PMTiles), MapTiler, Stadia, Google |
| Geocodificación | Texto ↔ coordenadas | Google Geocoding, Pelias, Nominatim propio |
| Rutas | Cálculo de rutas/ETA | Google Routes, Valhalla, OSRM |
| Map-matching | Ajustar trayectorias a la red vial | Valhalla Meili (post-GA, servidor, F13) |

## 2. Restricciones de licencia que condicionan el diseño
- Los términos específicos de servicio de Google Maps Platform (versión consultada: última modificación 10-jun-2026) establecen que el contenido de Geocoding, Directions, Distance Matrix, Geolocation, Places, Roads, Routes y Route Optimization **no puede usarse junto con un mapa que no sea de Google** (apartados 3.2, 4.2, 5.2, 6.2, 7.2, 14.2, 17.2, 18.2 y 19.2 de esos términos), y limitan el caché de latitud/longitud de esos servicios a 30 días con excepciones acotadas (apartados 6.3 y 19.3). Fuente: https://cloud.google.com/maps-platform/terms/maps-service-terms
- Los servidores comunitarios de OpenStreetMap (`tile.openstreetmap.org`, `nominatim.openstreetmap.org`) tienen políticas de uso que **no permiten uso intensivo en producción**; para producción se usan teselas propias o un proveedor comercial.
- Toda pila que use datos de OpenStreetMap DEBE mostrar la atribución "© OpenStreetMap contributors" (ODbL), además de la del proveedor de teselas.
- Los términos cambian: el PR que añade o actualiza un adaptador DEBE volver a verificarlos y actualizar la §4 con la fecha de consulta (FR-MAP-006).

## 3. Interfaces (paquete `geostream_maps`, normativas)

```dart
enum ProviderId { google, maplibre, mapbox, here, selfHosted, osmCommunity }

abstract interface class MapRenderer {
  ProviderId get provider;
  /// Widget de mapa que pinta los miembros de una SessionView.
  Widget buildMap({
    required Stream<SessionView> sessionView,
    MemberMarkerStyle Function(MemberView member)? markerStyle,
    CameraPolicy camera = const CameraPolicy.fitMembers(),
    bool interpolate = true,
    bool showAccuracyCircle = true,
    String? ownMemberId,
  });
}

abstract interface class GeocodingProvider {
  ProviderId get provider;
  Future<List<GeocodeResult>> search(String query, {GeoBounds? bias, String? language});
  Future<List<GeocodeResult>> reverse(double latitude, double longitude, {String? language});
}

abstract interface class RoutingProvider {
  ProviderId get provider;
  Future<RouteResult> route(List<Waypoint> waypoints, {TravelMode mode = TravelMode.driving});
}

final class ProviderStack {
  /// Lanza ConfigurationException si la combinación está prohibida (§4).
  factory ProviderStack({
    required MapRenderer renderer,
    GeocodingProvider? geocoding,
    RoutingProvider? routing,
    bool allowCommunityServers = false, // solo desarrollo
  });
  MapRenderer get renderer;
  GeocodingProvider? get geocoding;
  RoutingProvider? get routing;
}
```

Reglas:
- Las implementaciones viven en paquetes separados; `geostream_maps` no depende de ningún SDK de mapas.
- Los errores de proveedores se envuelven en `ProviderException` y **nunca** afectan a la compartición ni al stream.
- Los resultados de geocodificación y rutas no se envían a GeoStream ni se cachean más allá de lo que permitan los términos del proveedor.

## 4. Reglas de compatibilidad de `ProviderStack`

| Regla | Motivo |
|---|---|
| Si `geocoding.provider == google` o `routing.provider == google` → `renderer.provider` DEBE ser `google` | Términos de Google (§2) |
| `osmCommunity` en cualquier pieza → solo si `allowCommunityServers == true`; en builds de release se registra advertencia | Políticas de uso de OSM |
| `mapbox`, `here` → combinaciones a definir al crear su adaptador, tras revisar sus términos | Pendiente |

Matriz de proveedores (consulta inicial; revalidar en F9):

| Proveedor | Pieza | Licencia/términos | Costo | Atribución | Observaciones |
|---|---|---|---|---|---|
| MapLibre GL | Renderer | Código abierto (verificar licencia exacta del paquete Flutter en F9) | Gratis | Según teselas | Requiere una fuente de teselas y un estilo |
| Teselas propias (OpenMapTiles / PMTiles) | Datos | ODbL (datos OSM) + licencia del esquema | Infraestructura propia | "© OpenStreetMap contributors" | Mejor opción de privacidad |
| MapTiler / Stadia / similares | Datos | Comercial | Por uso | Proveedor + OSM | Clave de cliente restringida por app |
| Google Maps | Renderer + datos + geocoding + rutas | Términos de Google Maps Platform | Por uso (verificar precios vigentes) | Google | No mezclar con mapas no-Google |
| Pelias / Nominatim propio | Geocoding | Código abierto | Infraestructura propia | OSM | Nominatim público no apto para producción |
| Valhalla / OSRM | Rutas | Código abierto | Infraestructura propia | OSM | Post-GA para servidor |

## 5. Adaptadores

### 5.1 `geostream_maps_maplibre` (MUST, F9)
- Configuración: `styleUrl` obligatorio (estilo de las teselas elegidas) y, si el proveedor la requiere, una clave de cliente restringida por identificador de app.
- Atribución visible siempre; no se puede ocultar por configuración.
- Marcadores por miembro, círculo de precisión opcional (`horizontal_accuracy_m`), estilo atenuado cuando `freshness == stale`, retirada inmediata cuando el miembro deja de compartir.

### 5.2 `geostream_maps_google` (SHOULD, F9)
- Clave de API restringida por paquete Android / bundle iOS; nunca una clave de servidor.
- Requiere que el integrador acepte y cumpla los términos de Google Maps Platform y gestione la facturación.

## 6. Interpolación visual (FR-MAP-005)
- Solo dentro del renderer: anima el marcador desde la posición mostrada hasta la nueva `LocationUpdate` durante `min(Δt entre updates, 2 s)`, con interpolación lineal de latitud/longitud.
- **Nunca extrapola** más allá de la última posición recibida (sin predicción).
- Las posiciones interpoladas no salen del widget: no se exponen por API, no se publican y no se registran.
- Desactivable (`interpolate: false`). Si `freshness` pasa a `stale`, la animación se detiene en la última posición real.

## 7. Privacidad frente a proveedores de mapas
- El renderer pide teselas del área visible: el proveedor de teselas puede inferir **qué zona se está mirando**, que suele ser donde están las personas seguidas. Para casos sensibles (familia, seguridad), recomendar teselas propias.
- La geocodificación inversa de la ubicación de otra persona envía esa coordenada al proveedor: el SDK no lo hace automáticamente nunca; es una llamada explícita del integrador.
- Estas implicaciones se documentan en la guía de integración y deben reflejarse en la política de privacidad del integrador.

## 8. Servidor
En v1 el servidor no usa proveedores de mapas. Map-matching y rutas del lado servidor son pistas post-GA (F13) con ADR propio, aislados del camino crítico.
