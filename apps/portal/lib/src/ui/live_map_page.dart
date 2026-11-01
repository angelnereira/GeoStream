import 'dart:async';

import 'package:flutter/material.dart';
import 'package:http/http.dart' as http;
import 'package:maplibre_gl/maplibre_gl.dart';

import '../api/api_client.dart';
import '../api/models.dart';
import '../map/style_loader.dart';

/// Intervalo de actualización del mapa. El stream en vivo (WebSocket, Protobuf) es una fase posterior.
const Duration _refreshEvery = Duration(seconds: 5);

/// Mapa con las últimas posiciones de una sesión, sobre las teselas propias.
class LiveMapPage extends StatefulWidget {
  /// Crea la pantalla.
  const LiveMapPage({super.key, required this.client, required this.session});

  /// Cliente de solo lectura de la API.
  final ApiClient client;

  /// Sesión que se muestra.
  final SessionSummary session;

  @override
  State<LiveMapPage> createState() => _LiveMapPageState();
}

class _LiveMapPageState extends State<LiveMapPage> {
  late final Future<String> _style = loadOwnStyle(http.Client(), Uri.base);
  MapLibreMapController? _map;
  Timer? _timer;
  final Map<String, Circle> _circles = {};
  List<MemberPosition> _members = const [];
  bool _fitted = false;
  String? _error;
  DateTime? _updatedAt;

  @override
  void dispose() {
    _timer?.cancel();
    super.dispose();
  }

  void _onStyleLoaded() {
    unawaited(_refresh());
    _timer = Timer.periodic(_refreshEvery, (_) => unawaited(_refresh()));
  }

  Future<void> _refresh() async {
    final map = _map;
    if (map == null) return;
    try {
      final members = await widget.client.snapshot(widget.session.id);
      if (!mounted) return;
      await _draw(map, members);
      if (!mounted) return;
      setState(() {
        _members = members;
        _error = null;
        _updatedAt = DateTime.now().toUtc();
      });
    } on ApiKeyRejected {
      _timer?.cancel();
      if (!mounted) return;
      setState(
        () =>
            _error = 'La API key ya no es válida o no permite ver posiciones.',
      );
    } on ApiUnavailable {
      if (!mounted) return;
      setState(() => _error = 'Sin conexión con el servicio. Reintentando…');
    }
  }

  Future<void> _draw(
    MapLibreMapController map,
    List<MemberPosition> members,
  ) async {
    final seen = <String>{};
    for (final member in members) {
      seen.add(member.memberId);
      final at = LatLng(member.latitude, member.longitude);
      final existing = _circles[member.memberId];
      if (existing == null) {
        _circles[member.memberId] = await map.addCircle(
          CircleOptions(
            geometry: at,
            circleRadius: 9,
            circleColor: '#0b6e4f',
            circleStrokeColor: '#ffffff',
            circleStrokeWidth: 2,
          ),
        );
      } else {
        await map.updateCircle(existing, CircleOptions(geometry: at));
      }
    }
    for (final gone
        in _circles.keys.where((id) => !seen.contains(id)).toList()) {
      final circle = _circles.remove(gone);
      if (circle != null) await map.removeCircle(circle);
    }
    if (!_fitted && members.isNotEmpty) {
      _fitted = true;
      final first = members.first;
      await map.animateCamera(
        CameraUpdate.newLatLngZoom(LatLng(first.latitude, first.longitude), 14),
      );
    }
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: Text(widget.session.id)),
      body: Column(
        children: [
          Expanded(
            child: FutureBuilder<String>(
              future: _style,
              builder: (context, snapshot) {
                if (snapshot.hasError) {
                  return const Center(
                    child: Text('No pudimos cargar el mapa propio.'),
                  );
                }
                final style = snapshot.data;
                if (style == null) {
                  return const Center(child: CircularProgressIndicator());
                }
                return Stack(
                  children: [
                    MapLibreMap(
                      styleString: style,
                      initialCameraPosition: const CameraPosition(
                        target: LatLng(8.9824, -79.5199),
                        zoom: 11,
                      ),
                      onMapCreated: (controller) => _map = controller,
                      onStyleLoadedCallback: _onStyleLoaded,
                    ),
                  ],
                );
              },
            ),
          ),
          _MemberPanel(members: _members, error: _error, updatedAt: _updatedAt),
        ],
      ),
    );
  }
}

class _MemberPanel extends StatelessWidget {
  const _MemberPanel({
    required this.members,
    required this.error,
    required this.updatedAt,
  });

  final List<MemberPosition> members;
  final String? error;
  final DateTime? updatedAt;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Material(
      elevation: 4,
      child: SizedBox(
        height: 180,
        child: ListView(
          padding: const EdgeInsets.all(12),
          children: [
            if (error != null)
              Text(error!, style: TextStyle(color: theme.colorScheme.error)),
            Text(
              updatedAt == null
                  ? 'Esperando posiciones…'
                  : '${members.length} miembros compartiendo · se actualiza '
                        'cada ${_refreshEvery.inSeconds} s',
              style: theme.textTheme.labelLarge,
            ),
            if (updatedAt != null && members.isEmpty)
              const Text(
                'Nadie comparte ubicación ahora. Compartir es opcional y '
                'siempre lo activa cada persona.',
              ),
            Padding(
              padding: const EdgeInsets.only(top: 4),
              child: Text(
                'Mapa: $mapAttribution',
                style: theme.textTheme.bodySmall,
              ),
            ),
            for (final member in members)
              ListTile(
                dense: true,
                leading: const Icon(Icons.circle, size: 14),
                title: Text(member.label),
                subtitle: Text(
                  'Medida hace ${_age(member.ageMs)} · precisión '
                  '±${member.accuracyMeters.round()} m',
                ),
              ),
          ],
        ),
      ),
    );
  }

  static String _age(int ms) {
    final seconds = (ms / 1000).round();
    if (seconds < 60) return '$seconds s';
    return '${(seconds / 60).round()} min';
  }
}
