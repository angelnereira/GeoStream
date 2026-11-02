import 'package:flutter/material.dart';

import '../../console/console_api.dart';
import '../../platform/link_opener.dart';
import 'auth_view.dart';
import 'messages.dart';
import 'projects.dart';

/// La cuenta: correo, cambio de contraseña, cierre de sesión y borrado.
class AccountView extends StatefulWidget {
  /// Crea la vista.
  const AccountView({
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
  State<AccountView> createState() => _AccountViewState();
}

class _AccountViewState extends State<AccountView> {
  final TextEditingController _current = TextEditingController();
  final TextEditingController _next = TextEditingController();
  final TextEditingController _confirm = TextEditingController();
  late bool _hasPassword = widget.session.hasPassword;
  String? _problem;
  String? _done;
  bool _busy = false;

  @override
  void dispose() {
    _current.dispose();
    _next.dispose();
    _confirm.dispose();
    super.dispose();
  }

  Future<void> _change() async {
    if (_next.text != _confirm.text) {
      setState(() {
        _problem = 'Las dos contraseñas nuevas no coinciden.';
        _done = null;
      });
      return;
    }
    setState(() {
      _busy = true;
      _problem = null;
      _done = null;
    });
    try {
      await widget.api.changePassword(_current.text, _next.text);
      if (!mounted) return;
      _current.clear();
      _next.clear();
      _confirm.clear();
      setState(() {
        _hasPassword = true;
        _done =
            'Contraseña cambiada. Cerramos tus otras sesiones por seguridad.';
      });
    } on ConsoleApiException catch (e) {
      if (e.isUnauthenticated) {
        widget.onExpired();
      } else if (mounted) {
        setState(() => _problem = messageFor(e));
      }
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _delete() async {
    final typed = TextEditingController();
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('¿Borrar tu cuenta?'),
        content: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            const Text(
              'Se borran tu correo, tu contraseña y tus sesiones. No se puede '
              'deshacer. Si eres el único propietario de una organización con '
              'proyectos activos, antes tendrás que deshabilitarlos o transferirlos.',
            ),
            const SizedBox(height: 12),
            TextField(
              controller: typed,
              decoration: const InputDecoration(
                labelText: 'Escribe BORRAR para confirmar',
                border: OutlineInputBorder(),
              ),
            ),
          ],
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context, false),
            child: const Text('Cancelar'),
          ),
          FilledButton(
            onPressed: () =>
                Navigator.pop(context, typed.text.trim() == 'BORRAR'),
            child: const Text('Borrar cuenta'),
          ),
        ],
      ),
    );
    typed.dispose();
    if (confirmed != true) return;
    try {
      await widget.api.deleteAccount();
      widget.onDeleted();
    } on ConsoleApiException catch (e) {
      if (e.isUnauthenticated) {
        widget.onExpired();
      } else if (mounted) {
        setState(() => _problem = messageFor(e));
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return PageBody(
      children: [
        Text('Cuenta', style: theme.textTheme.headlineSmall),
        const SizedBox(height: 12),
        Card(
          child: ListTile(
            leading: const Icon(Icons.person_outline),
            title: Text(widget.session.email),
            subtitle: const Text('Correo de la cuenta'),
          ),
        ),
        Card(
          child: Padding(
            padding: const EdgeInsets.all(16),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  _hasPassword
                      ? 'Cambiar la contraseña'
                      : 'Fija una contraseña',
                  style: theme.textTheme.titleSmall,
                ),
                const SizedBox(height: 4),
                Text(
                  _hasPassword
                      ? 'Al cambiarla cerramos tus otras sesiones.'
                      : 'Tu cuenta aún no tiene contraseña. Fíjala para poder '
                            'entrar con ella.',
                ),
                const SizedBox(height: 12),
                if (_hasPassword) ...[
                  PasswordField(
                    controller: _current,
                    label: 'Contraseña actual',
                  ),
                  const SizedBox(height: 12),
                ],
                PasswordField(
                  controller: _next,
                  label: 'Contraseña nueva (mínimo 10 caracteres)',
                  newPassword: true,
                ),
                const SizedBox(height: 12),
                PasswordField(
                  controller: _confirm,
                  label: 'Repite la contraseña nueva',
                  newPassword: true,
                  onSubmitted: _change,
                ),
                if (_problem != null)
                  Padding(
                    padding: const EdgeInsets.only(top: 12),
                    child: Text(
                      _problem!,
                      style: TextStyle(color: theme.colorScheme.error),
                    ),
                  ),
                if (_done != null)
                  Padding(
                    padding: const EdgeInsets.only(top: 12),
                    child: Text(_done!),
                  ),
                const SizedBox(height: 12),
                FilledButton(
                  onPressed: _busy ? null : _change,
                  child: Text(_busy ? 'Un momento…' : 'Guardar contraseña'),
                ),
              ],
            ),
          ),
        ),
        Card(
          child: ListTile(
            leading: const Icon(Icons.logout),
            title: const Text('Cerrar sesión'),
            subtitle: const Text('Esta sesión se cierra en el servidor.'),
            onTap: widget.onSignOut,
          ),
        ),
        Card(
          child: ListTile(
            leading: Icon(Icons.delete_outline, color: theme.colorScheme.error),
            title: Text(
              'Borrar mi cuenta',
              style: TextStyle(color: theme.colorScheme.error),
            ),
            subtitle: const Text('Borra tu correo y tus sesiones.'),
            onTap: _delete,
          ),
        ),
      ],
    );
  }
}

/// Enlaces a la documentación publicada.
class DocsView extends StatelessWidget {
  /// Crea la vista.
  const DocsView({super.key});

  @override
  Widget build(BuildContext context) {
    return PageBody(
      children: [
        Text('Documentación', style: Theme.of(context).textTheme.headlineSmall),
        const SizedBox(height: 8),
        const Text(
          'Todo lo que necesitas para integrar GeoStream en tu aplicación.',
        ),
        const SizedBox(height: 8),
        for (final (label, path) in const [
          ('Guía de integración', '/docs/15_INTEGRATION_GUIDE.html'),
          ('Referencia de la API REST', '/docs/api.html'),
          (
            'Protocolo de stream en tiempo real',
            '/docs/08_STREAM_PROTOCOL.html',
          ),
          (
            'Validación y catálogo de errores',
            '/docs/09_VALIDATION_ERRORS.html',
          ),
          ('API REST: semántica', '/docs/07_REST_API.html'),
          ('Contrato OpenAPI', '/contracts/geostream.v1.yaml'),
        ])
          Card(
            child: ListTile(
              leading: const Icon(Icons.menu_book_outlined),
              title: Text(label),
              trailing: const Icon(Icons.open_in_new, size: 18),
              onTap: () => openLink(path),
            ),
          ),
      ],
    );
  }
}
