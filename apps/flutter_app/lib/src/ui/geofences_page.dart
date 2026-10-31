import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../geo/geofence.dart';
import '../geo/geofence_evaluator.dart';
import '../location/tracking_controller.dart';
import 'geofence_editor_page.dart';

/// Lista de geocercas locales con su estado y acciones.
class GeofencesPage extends StatelessWidget {
  /// Crea la página.
  const GeofencesPage({super.key, required this.controller});

  /// Estado compartido.
  final TrackingController controller;

  Future<void> _edit(BuildContext context, [LocalGeofence? fence]) async {
    final result = await Navigator.of(context).push<LocalGeofence>(
      MaterialPageRoute(
        builder: (_) => GeofenceEditorPage(
          initial: fence,
          sessionId: controller.sessionId,
          center: controller.lastFix?.point,
        ),
      ),
    );
    if (result != null) await controller.upsertFence(result);
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      floatingActionButton: FloatingActionButton.extended(
        onPressed: () => _edit(context),
        icon: const Icon(Icons.add),
        label: const Text('Nueva geocerca'),
      ),
      body: ListenableBuilder(
        listenable: controller,
        builder: (context, _) {
          final fences = controller.fences;
          return ListView(
            padding: const EdgeInsets.only(bottom: 96),
            children: [
              const ListTile(
                title: Text('Geocercas'),
                subtitle: Text(
                  'Se evalúan en el dispositivo como vista previa. La '
                  'transición oficial la decidirá el servidor cuando exista '
                  'POST /v1/geofences (fase F11).',
                ),
              ),
              if (fences.isEmpty)
                const Padding(
                  padding: EdgeInsets.all(24),
                  child: Text('Aún no hay geocercas. Crea una con el botón +.'),
                ),
              for (final f in fences)
                ListTile(
                  leading: Icon(switch (f.shape) {
                    CircleShape() => Icons.circle_outlined,
                    PolygonShape() => Icons.pentagon_outlined,
                  }),
                  title: Text(f.name),
                  subtitle: Text(_describe(f, controller.fenceState(f.id))),
                  onTap: () => _edit(context, f),
                  trailing: PopupMenuButton<String>(
                    onSelected: (action) async {
                      if (action == 'json') {
                        await Clipboard.setData(
                          ClipboardData(
                            text: const JsonEncoder.withIndent('  ')
                                .convert(f.toCreateRequest()),
                          ),
                        );
                        if (context.mounted) {
                          ScaffoldMessenger.of(context).showSnackBar(
                            const SnackBar(
                              content: Text('CreateGeofenceRequest copiado'),
                            ),
                          );
                        }
                      } else if (action == 'delete') {
                        await controller.deleteFence(f.id);
                      }
                    },
                    itemBuilder: (_) => const [
                      PopupMenuItem(value: 'json', child: Text('Copiar JSON')),
                      PopupMenuItem(value: 'delete', child: Text('Borrar')),
                    ],
                  ),
                ),
            ],
          );
        },
      ),
    );
  }

  static String _describe(LocalGeofence f, FenceState state) {
    final shape = switch (f.shape) {
      CircleShape(:final radiusM) => 'Círculo de ${radiusM.round()} m',
      PolygonShape(:final ring) => 'Polígono de ${ring.length - 1} vértices',
    };
    final where = switch (state) {
      FenceState.inside => 'dentro',
      FenceState.outside => 'fuera',
      FenceState.unknown => 'sin determinar',
    };
    return '$shape · $where';
  }
}
