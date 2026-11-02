import 'package:flutter/material.dart';

import '../console/console_api.dart';
import '../platform/session_store.dart';
import 'console/account_view.dart';
import 'console/auth_view.dart';
import 'console/projects.dart';

/// Botón «Inicio» cuando la consola es la primera pantalla (recarga o enlace directo) y no hay a dónde
/// volver; si se puede volver, el propio AppBar pone la flecha.
Widget? _homeButton(BuildContext context) => Navigator.of(context).canPop()
    ? null
    : IconButton(
        tooltip: 'Inicio',
        icon: const Icon(Icons.home_outlined),
        onPressed: () => Navigator.of(context).pushReplacementNamed('/'),
      );

/// La consola de desarrolladores: acceso con contraseña y código, y un panel para administrar proyectos y
/// API keys (FR-CON-001, FR-CON-003, FR-CON-004, FR-CON-012, FR-CON-013).
///
/// La sesión sobrevive a recargar la página (almacén de la pestaña, ADR-0037): al arrancar se retoma el token
/// guardado y se confirma con el servidor; si ya no vale, se pide entrar de nuevo.
class DeveloperConsole extends StatefulWidget {
  /// Crea la consola.
  const DeveloperConsole({super.key, required this.api, this.store});

  /// Cliente de `/v1/console`.
  final ConsoleApi api;

  /// Dónde recordar la sesión entre recargas; por defecto el de la plataforma.
  final SessionStore? store;

  @override
  State<DeveloperConsole> createState() => _DeveloperConsoleState();
}

class _DeveloperConsoleState extends State<DeveloperConsole> {
  late final SessionStore _store = widget.store ?? createSessionStore();
  ConsoleSessionInfo? _session;
  bool _booting = true;
  String? _notice;

  @override
  void initState() {
    super.initState();
    _resume();
  }

  Future<void> _resume() async {
    final token = _store.read();
    if (token != null) {
      try {
        final info = await widget.api.restore(token);
        if (!mounted) return;
        setState(() {
          _session = info;
          _booting = false;
        });
        return;
      } on ConsoleApiException catch (e) {
        _store.clear();
        if (mounted && e.isUnauthenticated) {
          _notice = 'Tu sesión caducó. Entra de nuevo.';
        }
      }
    }
    if (mounted) setState(() => _booting = false);
  }

  void _signedIn(ConsoleSessionInfo info) {
    _store.write(info.token);
    setState(() {
      _session = info;
      _notice = null;
    });
  }

  void _signedOut({String? notice}) {
    _store.clear();
    setState(() {
      _session = null;
      _notice = notice;
    });
  }

  Future<void> _logout() async {
    try {
      await widget.api.logout();
    } on ConsoleApiException {
      // La sesión local se olvida igual; si el servidor no respondió, caducará sola.
    }
    if (mounted) _signedOut();
  }

  @override
  Widget build(BuildContext context) {
    if (_booting) {
      return const Scaffold(body: Center(child: CircularProgressIndicator()));
    }
    final session = _session;
    if (session == null) {
      return Scaffold(
        appBar: AppBar(
          leading: _homeButton(context),
          title: const Text('Consola de desarrolladores'),
        ),
        body: Column(
          children: [
            if (_notice != null)
              MaterialBanner(
                content: Text(_notice!),
                actions: [
                  TextButton(
                    onPressed: () => setState(() => _notice = null),
                    child: const Text('Entendido'),
                  ),
                ],
              ),
            Expanded(
              child: AuthView(api: widget.api, onSignedIn: _signedIn),
            ),
          ],
        ),
      );
    }
    return ConsoleShell(
      api: widget.api,
      session: session,
      onSignOut: _logout,
      onExpired: () => _signedOut(notice: 'Tu sesión caducó. Entra de nuevo.'),
      onDeleted: () => _signedOut(notice: 'Tu cuenta se borró.'),
    );
  }
}

/// La estructura del panel: navegación lateral en pantallas anchas y barra inferior en las estrechas.
class ConsoleShell extends StatefulWidget {
  /// Crea la estructura.
  const ConsoleShell({
    super.key,
    required this.api,
    required this.session,
    required this.onSignOut,
    required this.onExpired,
    required this.onDeleted,
  });

  /// Cliente de la consola.
  final ConsoleApi api;

  /// Sesión abierta.
  final ConsoleSessionInfo session;

  /// La persona pide cerrar la sesión.
  final VoidCallback onSignOut;

  /// La sesión dejó de valer.
  final VoidCallback onExpired;

  /// La cuenta se borró.
  final VoidCallback onDeleted;

  @override
  State<ConsoleShell> createState() => _ConsoleShellState();
}

class _ConsoleShellState extends State<ConsoleShell> {
  int _index = 0;

  static const List<({String label, IconData icon})> _destinations = [
    (label: 'Proyectos', icon: Icons.folder_outlined),
    (label: 'Documentación', icon: Icons.menu_book_outlined),
    (label: 'Cuenta', icon: Icons.person_outline),
  ];

  @override
  Widget build(BuildContext context) {
    final body = IndexedStack(
      index: _index,
      children: [
        OrganizationsView(api: widget.api, onExpired: widget.onExpired),
        const DocsView(),
        AccountView(
          api: widget.api,
          session: widget.session,
          onSignOut: widget.onSignOut,
          onExpired: widget.onExpired,
          onDeleted: widget.onDeleted,
        ),
      ],
    );
    return LayoutBuilder(
      builder: (context, constraints) {
        final wide = constraints.maxWidth >= 720;
        return Scaffold(
          appBar: AppBar(
            leading: _homeButton(context),
            title: const Text('Consola de GeoStream'),
            actions: [
              if (constraints.maxWidth >= 480)
                Padding(
                  padding: const EdgeInsets.symmetric(horizontal: 8),
                  child: Center(child: Text(widget.session.email)),
                ),
              IconButton(
                tooltip: 'Cerrar sesión',
                icon: const Icon(Icons.logout),
                onPressed: widget.onSignOut,
              ),
            ],
          ),
          body: wide
              ? Row(
                  children: [
                    NavigationRail(
                      selectedIndex: _index,
                      labelType: NavigationRailLabelType.all,
                      onDestinationSelected: (i) => setState(() => _index = i),
                      destinations: [
                        for (final d in _destinations)
                          NavigationRailDestination(
                            icon: Icon(d.icon),
                            label: Text(d.label),
                          ),
                      ],
                    ),
                    const VerticalDivider(width: 1),
                    Expanded(child: body),
                  ],
                )
              : body,
          bottomNavigationBar: wide
              ? null
              : NavigationBar(
                  selectedIndex: _index,
                  onDestinationSelected: (i) => setState(() => _index = i),
                  destinations: [
                    for (final d in _destinations)
                      NavigationDestination(icon: Icon(d.icon), label: d.label),
                  ],
                ),
        );
      },
    );
  }
}
