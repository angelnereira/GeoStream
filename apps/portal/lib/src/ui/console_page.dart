import 'package:flutter/material.dart';

import '../api/api_client.dart';
import '../api/models.dart';
import 'live_map_page.dart';

/// Crea un cliente de la API para una API key (inyectable en pruebas).
typedef ApiClientFactory = ApiClient Function(String apiKey);

/// Acceso con API key de solo lectura y lista de sesiones (FR-PRT-004, ADR-0034 §4).
///
/// La key se guarda solo en el estado de esta pantalla: nada en el almacenamiento del navegador, cookies ni
/// ningún otro almacenamiento. Al salir o recargar la página se pierde.
class ConsolePage extends StatefulWidget {
  /// Crea la pantalla.
  const ConsolePage({super.key, required this.createClient, this.mapBuilder});

  /// Construye el cliente de la API con la key que escribe el usuario.
  final ApiClientFactory createClient;

  /// Construye la pantalla del mapa; las pruebas la sustituyen (MapLibre no corre fuera del navegador).
  final Widget Function(ApiClient client, SessionSummary session)? mapBuilder;

  @override
  State<ConsolePage> createState() => _ConsolePageState();
}

class _ConsolePageState extends State<ConsolePage> {
  final TextEditingController _key = TextEditingController();
  ApiClient? _client;
  List<SessionSummary>? _sessions;
  String? _error;
  bool _loading = false;

  @override
  void dispose() {
    _key.dispose();
    super.dispose();
  }

  Future<void> _enter() async {
    final apiKey = _key.text.trim();
    if (apiKey.isEmpty) return;
    setState(() {
      _loading = true;
      _error = null;
    });
    final client = widget.createClient(apiKey);
    try {
      final sessions = await client.listSessions();
      if (!mounted) return;
      setState(() {
        _client = client;
        _sessions = sessions;
        _loading = false;
      });
      // La key ya está en el cliente: se borra del campo de texto.
      _key.clear();
    } on ApiKeyRejected {
      if (!mounted) return;
      setState(() {
        _error =
            'La API key no fue aceptada. Revisa que sea correcta, que '
            'no esté revocada y que tenga el permiso de lectura de sesiones.';
        _loading = false;
      });
    } on ApiUnavailable {
      if (!mounted) return;
      setState(() {
        _error = 'No pudimos consultar el servicio. Inténtalo de nuevo en un momento.';
        _loading = false;
      });
    }
  }

  void _leave() {
    setState(() {
      _client = null;
      _sessions = null;
      _error = null;
    });
  }

  @override
  Widget build(BuildContext context) {
    final sessions = _sessions;
    return Scaffold(
      appBar: AppBar(
        title: const Text('Mis sesiones'),
        actions: [
          if (sessions != null)
            TextButton(onPressed: _leave, child: const Text('Salir')),
        ],
      ),
      body: Center(
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 720),
          child: Padding(
            padding: const EdgeInsets.all(16),
            child: sessions == null
                ? _keyForm(context)
                : _sessionList(sessions),
          ),
        ),
      ),
    );
  }

  Widget _keyForm(BuildContext context) {
    final theme = Theme.of(context);
    return ListView(
      shrinkWrap: true,
      children: [
        Text('Entra con tu API key', style: theme.textTheme.headlineSmall),
        const SizedBox(height: 8),
        const Text(
          'Usa una key de solo lectura, con permiso para ver sesiones y '
          'posiciones. Se guarda solo mientras esta pestaña esté abierta: '
          'al recargar la página se pierde. Este portal solo lee; nunca '
          'escribe en tu proyecto.',
        ),
        const SizedBox(height: 16),
        TextField(
          controller: _key,
          obscureText: true,
          enableSuggestions: false,
          autocorrect: false,
          autofillHints: const <String>[],
          decoration: const InputDecoration(
            labelText: 'API key',
            border: OutlineInputBorder(),
          ),
          onSubmitted: (_) => _enter(),
        ),
        if (_error != null) ...[
          const SizedBox(height: 12),
          Text(_error!, style: TextStyle(color: theme.colorScheme.error)),
        ],
        const SizedBox(height: 16),
        FilledButton(
          onPressed: _loading ? null : _enter,
          child: Text(_loading ? 'Entrando…' : 'Entrar'),
        ),
      ],
    );
  }

  Widget _sessionList(List<SessionSummary> sessions) {
    if (sessions.isEmpty) {
      return const Center(
        child: Text('Tu proyecto no tiene sesiones activas.'),
      );
    }
    return ListView(
      children: [
        for (final session in sessions)
          Card(
            child: ListTile(
              title: Text(session.id),
              subtitle: Text(
                '${session.mode} · ${session.activeMemberCount} miembros activos',
              ),
              trailing: const Icon(Icons.map_outlined),
              onTap: () {
                final client = _client;
                if (client == null) return;
                Navigator.of(context).push(
                  MaterialPageRoute<void>(
                    builder: (_) =>
                        widget.mapBuilder?.call(client, session) ??
                        LiveMapPage(client: client, session: session),
                  ),
                );
              },
            ),
          ),
      ],
    );
  }
}
