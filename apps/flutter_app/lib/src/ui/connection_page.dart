import 'package:flutter/material.dart';

import '../location/tracking_controller.dart';

/// Servidor, token de dispositivo, sesión y compartición.
class ConnectionPage extends StatefulWidget {
  /// Crea la página.
  const ConnectionPage({super.key, required this.controller});

  /// Estado compartido.
  final TrackingController controller;

  @override
  State<ConnectionPage> createState() => _ConnectionPageState();
}

class _ConnectionPageState extends State<ConnectionPage> {
  late final TextEditingController _url = TextEditingController(
    text: widget.controller.store.baseUrl,
  );
  final TextEditingController _token = TextEditingController();
  final TextEditingController _session = TextEditingController();

  @override
  void dispose() {
    _url.dispose();
    _token.dispose();
    _session.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final c = widget.controller;
    return ListenableBuilder(
      listenable: c,
      builder: (context, _) => ListView(
        padding: const EdgeInsets.all(16),
        children: [
          Text(
            'GeoStream Tester',
            style: Theme.of(context).textTheme.headlineSmall,
          ),
          const SizedBox(height: 12),
          TextField(
            controller: _url,
            keyboardType: TextInputType.url,
            decoration: const InputDecoration(
              labelText: 'Servidor',
              border: OutlineInputBorder(),
            ),
          ),
          const SizedBox(height: 12),
          TextField(
            controller: _token,
            obscureText: true,
            autocorrect: false,
            enableSuggestions: false,
            decoration: const InputDecoration(
              labelText: 'Token de dispositivo (no se guarda)',
              helperText: 'Lo emite tu backend con POST /v1/tokens',
              border: OutlineInputBorder(),
            ),
          ),
          const SizedBox(height: 8),
          FilledButton.icon(
            onPressed: c.busy ? null : () => c.connect(_url.text, _token.text),
            icon: const Icon(Icons.login),
            label: Text(c.connected ? 'Reconectar' : 'Conectar'),
          ),
          const Divider(height: 32),
          Text('Sesión: ${c.sessionId ?? '—'}'),
          const SizedBox(height: 8),
          Row(
            children: [
              Expanded(
                child: TextField(
                  controller: _session,
                  onChanged: (_) => setState(() {}),
                  decoration: const InputDecoration(
                    labelText: 'ID de sesión existente (ses_…)',
                    border: OutlineInputBorder(),
                  ),
                ),
              ),
              const SizedBox(width: 8),
              OutlinedButton(
                onPressed: c.connected && !c.sharing && _session.text.isNotEmpty
                    ? () => c.useSession(_session.text)
                    : null,
                child: const Text('Usar'),
              ),
            ],
          ),
          const SizedBox(height: 8),
          OutlinedButton.icon(
            onPressed: c.connected && !c.busy && !c.sharing
                ? c.createSession
                : null,
            icon: const Icon(Icons.add),
            label: const Text('Crear sesión nueva'),
          ),
          const SizedBox(height: 16),
          SwitchListTile(
            title: const Text('Compartir mi ubicación'),
            subtitle: const Text('Apagado por defecto. Solo tú lo activas.'),
            value: c.sharing,
            onChanged: c.connected && c.sessionId != null && !c.busy
                ? (on) => on ? c.startSharing() : c.stopSharing()
                : null,
          ),
          _Stats(controller: c),
          const Divider(height: 32),
          Text('Registro', style: Theme.of(context).textTheme.titleMedium),
          for (final line in c.log.take(50))
            Text(
              '${TimeOfDay.fromDateTime(line.at).format(context)}  ${line.message}',
              style: Theme.of(context).textTheme.bodySmall,
            ),
        ],
      ),
    );
  }
}

class _Stats extends StatelessWidget {
  const _Stats({required this.controller});

  final TrackingController controller;

  @override
  Widget build(BuildContext context) {
    final fix = controller.lastFix;
    return Card(
      child: Padding(
        padding: const EdgeInsets.all(12),
        child: Wrap(
          spacing: 24,
          runSpacing: 8,
          children: [
            Text('Aceptadas: ${controller.accepted}'),
            Text('Rechazadas: ${controller.rejected}'),
            Text('Descartadas: ${controller.dropped}'),
            Text(
              fix == null
                  ? 'Precisión: —'
                  : 'Precisión: ${fix.accuracyM.toStringAsFixed(1)} m',
            ),
          ],
        ),
      ),
    );
  }
}
