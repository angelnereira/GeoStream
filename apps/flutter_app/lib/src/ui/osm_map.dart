import 'package:flutter/material.dart';
import 'package:maplibre_gl/maplibre_gl.dart';

import '../map/osm_style.dart';

/// Mapa MapLibre con teselas OSM y atribución ODbL siempre visible.
class OsmMap extends StatelessWidget {
  /// Crea el mapa.
  const OsmMap({
    super.key,
    required this.onMapCreated,
    required this.onStyleLoaded,
    this.onMapClick,
    this.initial = const LatLng(8.9824, -79.5199),
  });

  /// Recibe el controlador nativo.
  final MapCreatedCallback onMapCreated;

  /// El estilo está listo para anotaciones.
  final VoidCallback onStyleLoaded;

  /// Toque en el mapa.
  final OnMapClickCallback? onMapClick;

  /// Centro inicial (por defecto, Ciudad de Panamá).
  final LatLng initial;

  @override
  Widget build(BuildContext context) {
    return Stack(
      children: [
        MapLibreMap(
          styleString: osmRasterStyle(),
          initialCameraPosition: CameraPosition(target: initial, zoom: 13),
          onMapCreated: onMapCreated,
          onStyleLoadedCallback: onStyleLoaded,
          onMapClick: onMapClick,
          compassEnabled: true,
        ),
        Positioned(
          right: 4,
          bottom: 4,
          child: Container(
            padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 2),
            color: Colors.white70,
            child: Text(
              usesCommunityServers
                  ? '$osmAttribution · servidor comunitario (solo pruebas)'
                  : osmAttribution,
              style: const TextStyle(fontSize: 11, color: Colors.black87),
            ),
          ),
        ),
      ],
    );
  }
}
