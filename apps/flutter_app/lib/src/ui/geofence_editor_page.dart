import 'dart:async';
import 'dart:math';

import 'package:flutter/material.dart';
import 'package:maplibre_gl/maplibre_gl.dart';

import '../geo/geodesy.dart';
import '../geo/geofence.dart';
import '../map/fence_drawing.dart';
import 'osm_map.dart';

enum _Mode { circle, polygon }

/// Editor sobre el mapa: toca para fijar el centro o añadir vértices.
class GeofenceEditorPage extends StatefulWidget {
  /// Crea el editor; [initial] para editar una existente.
  const GeofenceEditorPage({
    super.key,
    this.initial,
    this.sessionId,
    this.center,
  });

  /// Geocerca a editar, o `null` para una nueva.
  final LocalGeofence? initial;

  /// Sesión activa, que se asocia a la geocerca.
  final String? sessionId;

  /// Centro inicial del mapa (mi última posición).
  final GeoPoint? center;

  @override
  State<GeofenceEditorPage> createState() => _GeofenceEditorPageState();
}

class _GeofenceEditorPageState extends State<GeofenceEditorPage> {
  MapLibreMapController? _map;
  bool _styleReady = false;
  late _Mode _mode;
  GeoPoint? _center;
  double _radiusM = 150;
  List<GeoPoint> _vertices = [];
  late final TextEditingController _name;
  List<GeofenceViolation> _violations = const [];

  @override
  void initState() {
    super.initState();
    final f = widget.initial;
    _name = TextEditingController(text: f?.name ?? 'Geocerca');
    switch (f?.shape) {
      case CircleShape(:final center, :final radiusM):
        _mode = _Mode.circle;
        _center = center;
        _radiusM = radiusM;
      case PolygonShape(:final ring):
        _mode = _Mode.polygon;
        _vertices = ring.sublist(0, ring.length - 1);
      case null:
        _mode = _Mode.circle;
    }
  }

  @override
  void dispose() {
    _name.dispose();
    super.dispose();
  }

  GeofenceShape? get _shape => switch (_mode) {
    _Mode.circle => _center == null ? null : CircleShape(_center!, _radiusM),
    _Mode.polygon =>
      _vertices.length < 3 ? null : PolygonShape.fromVertices(_vertices),
  };

  Future<void> _redraw() async {
    final map = _map;
    if (map == null || !_styleReady) return;
    await clearAll(map);
    final shape = _shape;
    if (shape != null) await addFence(map, shape, color: '#ef6c00');
    if (_mode == _Mode.polygon) {
      for (final v in _vertices) {
        await map.addCircle(
          CircleOptions(
            geometry: toLatLng(v),
            circleRadius: 5,
            circleColor: '#ef6c00',
            circleStrokeColor: '#ffffff',
            circleStrokeWidth: 2,
          ),
        );
      }
    } else if (_center != null) {
      await map.addCircle(
        CircleOptions(
          geometry: toLatLng(_center!),
          circleRadius: 5,
          circleColor: '#ef6c00',
        ),
      );
    }
  }

  void _onTap(Point<double> _, LatLng at) {
    final p = GeoPoint(at.latitude, at.longitude);
    setState(() {
      if (_mode == _Mode.circle) {
        _center = p;
      } else if (_vertices.length < maxRingPoints - 1) {
        _vertices = [..._vertices, p];
      }
      _violations = const [];
    });
    unawaited(_redraw());
  }

  void _save() {
    final shape = _shape;
    if (shape == null) {
      setState(
        () => _violations = const [
          GeofenceViolation(
            'shape',
            'required',
            'Toca el mapa: el centro del círculo o al menos 3 vértices.',
          ),
        ],
      );
      return;
    }
    final fence = LocalGeofence(
      id:
          widget.initial?.id ??
          'local_${DateTime.now().microsecondsSinceEpoch.toRadixString(16)}',
      name: _name.text.trim(),
      shape: shape,
      sessionId: widget.sessionId ?? widget.initial?.sessionId,
    );
    final violations = fence.validate();
    if (violations.isNotEmpty) {
      setState(() => _violations = violations);
      return;
    }
    Navigator.of(context).pop(fence);
  }

  @override
  Widget build(BuildContext context) {
    final start =
        _center ??
        (_vertices.isNotEmpty ? _vertices.first : null) ??
        widget.center;
    return Scaffold(
      appBar: AppBar(
        title: Text(
          widget.initial == null ? 'Nueva geocerca' : 'Editar geocerca',
        ),
        actions: [
          IconButton(
            tooltip: 'Guardar',
            icon: const Icon(Icons.check),
            onPressed: _save,
          ),
        ],
      ),
      body: Column(
        children: [
          Padding(
            padding: const EdgeInsets.fromLTRB(12, 12, 12, 0),
            child: Column(
              children: [
                TextField(
                  controller: _name,
                  maxLength: maxNameLength,
                  decoration: const InputDecoration(
                    labelText: 'Nombre',
                    border: OutlineInputBorder(),
                  ),
                ),
                SegmentedButton<_Mode>(
                  segments: const [
                    ButtonSegment(
                      value: _Mode.circle,
                      icon: Icon(Icons.circle_outlined),
                      label: Text('Círculo'),
                    ),
                    ButtonSegment(
                      value: _Mode.polygon,
                      icon: Icon(Icons.pentagon_outlined),
                      label: Text('Polígono'),
                    ),
                  ],
                  selected: {_mode},
                  onSelectionChanged: (s) {
                    setState(() {
                      _mode = s.first;
                      _violations = const [];
                    });
                    unawaited(_redraw());
                  },
                ),
                if (_mode == _Mode.circle)
                  Row(
                    children: [
                      Text('Radio ${_radiusM.round()} m'),
                      Expanded(
                        child: Slider(
                          min: log(minRadiusM),
                          max: log(5000),
                          value: log(_radiusM.clamp(minRadiusM, 5000)),
                          onChanged: (v) {
                            setState(() => _radiusM = exp(v).roundToDouble());
                            unawaited(_redraw());
                          },
                        ),
                      ),
                    ],
                  )
                else
                  Row(
                    children: [
                      Expanded(
                        child: Text(
                          '${_vertices.length} vértices · toca para añadir',
                        ),
                      ),
                      TextButton(
                        onPressed: _vertices.isEmpty
                            ? null
                            : () {
                                setState(
                                  () => _vertices = _vertices.sublist(
                                    0,
                                    _vertices.length - 1,
                                  ),
                                );
                                unawaited(_redraw());
                              },
                        child: const Text('Deshacer'),
                      ),
                    ],
                  ),
                for (final v in _violations)
                  Text(
                    v.message,
                    style: TextStyle(
                      color: Theme.of(context).colorScheme.error,
                    ),
                  ),
              ],
            ),
          ),
          Expanded(
            child: OsmMap(
              initial: start == null
                  ? const LatLng(8.9824, -79.5199)
                  : toLatLng(start),
              onMapCreated: (m) => _map = m,
              onStyleLoaded: () {
                _styleReady = true;
                unawaited(_redraw());
              },
              onMapClick: _onTap,
            ),
          ),
        ],
      ),
    );
  }
}
