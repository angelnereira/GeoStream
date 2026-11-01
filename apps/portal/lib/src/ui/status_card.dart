import 'dart:async';

import 'package:flutter/material.dart';

import '../status/readiness.dart';
import '../status/status_client.dart';

/// Nombres legibles de las dependencias que informa la API.
const Map<String, String> _checkLabels = {
  'postgresql': 'Base de datos',
  'ephemeral': 'Estado en vivo',
  'bus': 'Distribución entre instancias',
};

/// Tarjeta con el estado de la API, consultado al abrir y cada [every].
class StatusCard extends StatefulWidget {
  /// Crea la tarjeta.
  const StatusCard({
    super.key,
    required this.client,
    this.every = const Duration(seconds: 30),
  });

  /// Origen de los datos.
  final StatusClient client;

  /// Intervalo entre consultas.
  final Duration every;

  @override
  State<StatusCard> createState() => _StatusCardState();
}

class _StatusCardState extends State<StatusCard> {
  ReadinessSnapshot? _snapshot;
  Timer? _timer;

  @override
  void initState() {
    super.initState();
    unawaited(_refresh());
    _timer = Timer.periodic(widget.every, (_) => unawaited(_refresh()));
  }

  @override
  void dispose() {
    _timer?.cancel();
    super.dispose();
  }

  Future<void> _refresh() async {
    final snapshot = await widget.client.fetch();
    if (!mounted) return;
    setState(() => _snapshot = snapshot);
  }

  @override
  Widget build(BuildContext context) {
    final snapshot = _snapshot;
    final theme = Theme.of(context);
    final (icon, label, color) = switch (snapshot) {
      null => (
        Icons.hourglass_empty,
        'Consultando…',
        theme.colorScheme.outline,
      ),
      ReadinessUnknown() => (
        Icons.help_outline,
        'Sin datos',
        theme.colorScheme.outline,
      ),
      ReadinessKnown(ready: true) => (
        Icons.check_circle,
        'Operativo',
        Colors.green.shade700,
      ),
      ReadinessKnown() => (Icons.error, 'Degradado', theme.colorScheme.error),
    };
    return Card(
      child: Padding(
        padding: const EdgeInsets.all(20),
        child: Semantics(
          container: true,
          label: 'Estado del servicio: $label',
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Row(
                children: [
                  Icon(icon, color: color),
                  const SizedBox(width: 8),
                  Text(label, style: theme.textTheme.titleLarge),
                ],
              ),
              const SizedBox(height: 12),
              if (snapshot is ReadinessKnown)
                for (final entry in snapshot.checks.entries)
                  _CheckRow(name: entry.key, state: entry.value),
              if (snapshot is ReadinessUnknown)
                Text(
                  'No pudimos consultar el servicio desde aquí. '
                  'Esto no significa que esté caído.',
                  style: theme.textTheme.bodyMedium,
                ),
              if (snapshot != null) ...[
                const SizedBox(height: 12),
                Text(
                  'Consultado ${_utc(snapshot.checkedAt)}. '
                  'Es el estado actual, no un porcentaje de disponibilidad.',
                  style: theme.textTheme.bodySmall,
                ),
              ],
            ],
          ),
        ),
      ),
    );
  }
}

class _CheckRow extends StatelessWidget {
  const _CheckRow({required this.name, required this.state});

  final String name;
  final CheckState state;

  @override
  Widget build(BuildContext context) {
    final (text, icon) = switch (state) {
      CheckState.ok => ('Operativo', Icons.check),
      CheckState.failing => ('Con fallos', Icons.close),
      CheckState.draining => ('Reiniciándose', Icons.sync),
      CheckState.notConfigured => ('No aplica', Icons.remove),
    };
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 2),
      child: Row(
        children: [
          Icon(icon, size: 16),
          const SizedBox(width: 8),
          Expanded(child: Text(_checkLabels[name] ?? name)),
          Text(text),
        ],
      ),
    );
  }
}

String _utc(DateTime time) {
  final t = time.toUtc();
  String two(int n) => n.toString().padLeft(2, '0');
  return '${t.year}-${two(t.month)}-${two(t.day)} '
      '${two(t.hour)}:${two(t.minute)}:${two(t.second)} UTC';
}
