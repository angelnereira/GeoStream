import 'dart:async';

import 'package:flutter/material.dart';
import 'package:maplibre_gl/maplibre_gl.dart';

import '../geo/geodesy.dart';
import '../location/tracking_controller.dart';
import '../map/fence_drawing.dart';
import 'osm_map.dart';

/// Mapa en vivo: mi posición, los miembros del snapshot y las geocercas.
class LiveMapPage extends StatefulWidget {
  /// Crea la página.
  const LiveMapPage({super.key, required this.controller});

  /// Estado compartido.
  final TrackingController controller;

  @override
  State<LiveMapPage> createState() => _LiveMapPageState();
}

class _LiveMapPageState extends State<LiveMapPage> {
  MapLibreMapController? _map;
  bool _styleReady = false;
  bool _drawing = false;
  bool _dirty = false;
  bool _follow = true;

  @override
  void initState() {
    super.initState();
    widget.controller.addListener(_scheduleRedraw);
  }

  @override
  void dispose() {
    widget.controller.removeListener(_scheduleRedraw);
    super.dispose();
  }

  void _scheduleRedraw() {
    _dirty = true;
    unawaited(_redraw());
  }

  Future<void> _redraw() async {
    final map = _map;
    if (map == null || !_styleReady || _drawing) return;
    _drawing = true;
    try {
      while (_dirty) {
        _dirty = false;
        final c = widget.controller;
        await clearAll(map);
        for (final f in c.fences) {
          await addFence(map, f.shape, color: fenceColor(c.fenceState(f.id)));
        }
        for (final m in c.members) {
          await addPosition(
            map,
            GeoPoint(m.latitude, m.longitude),
            m.accuracyM,
            color: '#6a1b9a',
          );
        }
        final fix = c.lastFix;
        if (fix != null) {
          await addPosition(map, fix.point, fix.accuracyM);
          if (_follow) {
            await map.animateCamera(
              CameraUpdate.newLatLng(toLatLng(fix.point)),
            );
          }
        }
      }
    } finally {
      _drawing = false;
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = widget.controller;
    return Stack(
      children: [
        OsmMap(
          onMapCreated: (m) => _map = m,
          onStyleLoaded: () {
            _styleReady = true;
            _scheduleRedraw();
          },
        ),
        Positioned(
          left: 12,
          top: 12,
          right: 12,
          child: ListenableBuilder(
            listenable: c,
            builder: (context, _) {
              final fix = c.lastFix;
              return Card(
                child: Padding(
                  padding: const EdgeInsets.all(10),
                  child: Row(
                    children: [
                      Icon(
                        c.sharing ? Icons.gps_fixed : Icons.gps_off,
                        color: c.sharing ? Colors.green : Colors.grey,
                      ),
                      const SizedBox(width: 8),
                      Expanded(
                        child: Text(
                          fix == null
                              ? (c.sharing
                                    ? 'Esperando posición…'
                                    : 'Sin compartir')
                              : '±${fix.accuracyM.toStringAsFixed(1)} m · '
                                    '${c.accepted} aceptadas · '
                                    '${c.members.length} en snapshot',
                        ),
                      ),
                      IconButton(
                        tooltip: 'Seguir mi posición',
                        icon: Icon(
                          _follow
                              ? Icons.my_location
                              : Icons.location_searching,
                        ),
                        onPressed: () => setState(() => _follow = !_follow),
                      ),
                    ],
                  ),
                ),
              );
            },
          ),
        ),
      ],
    );
  }
}
