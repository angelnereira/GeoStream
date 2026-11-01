import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../console/console_api.dart';
import '../platform/link_opener.dart';

/// Scopes que puede llevar una API key de aplicación (`docs/06` §3.4), con su explicación.
const Map<String, String> keyScopes = {
  'sessions:read': 'Leer sesiones y últimas posiciones',
  'sessions:write': 'Crear y cerrar sesiones',
  'members:write': 'Gestionar miembros',
  'invitations:write': 'Crear invitaciones',
  'tokens:mint': 'Emitir tokens para dispositivos',
  'stream:connect': 'Conectarse al stream en tiempo real',
  'locations:read': 'Leer ubicaciones (dato sensible)',
  'subjects:manage': 'Gestionar sujetos',
};

/// Consola de desarrolladores: registro, inicio de sesión y gestión de organizaciones, proyectos y API keys
/// (FR-CON-001 a FR-CON-004, FR-CON-012).
///
/// La sesión vive solo en memoria: al recargar la página hay que volver a entrar.
class DeveloperConsole extends StatefulWidget {
  /// Crea la consola.
  const DeveloperConsole({super.key, required this.api});

  /// Cliente de `/v1/console`.
  final ConsoleApi api;

  @override
  State<DeveloperConsole> createState() => _DeveloperConsoleState();
}

class _DeveloperConsoleState extends State<DeveloperConsole> {
  ConsoleSessionInfo? _session;

  Future<void> _logout() async {
    try {
      await widget.api.logout();
    } on ConsoleApiException {
      // La sesión local se olvida igual; si el servidor no respondió, caducará sola.
    }
    if (mounted) setState(() => _session = null);
  }

  void _expired() => setState(() => _session = null);

  @override
  Widget build(BuildContext context) {
    final session = _session;
    return Scaffold(
      appBar: AppBar(
        title: const Text('Consola de desarrolladores'),
        actions: [
          if (session != null) ...[
            Center(child: Text(session.email)),
            TextButton(onPressed: _logout, child: const Text('Salir')),
          ],
        ],
      ),
      body: Center(
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 720),
          child: Padding(
            padding: const EdgeInsets.all(16),
            child: session == null
                ? SignInForm(
                    api: widget.api,
                    onSignedIn: (info) => setState(() => _session = info),
                  )
                : OrganizationsView(api: widget.api, onExpired: _expired),
          ),
        ),
      ),
    );
  }
}

/// Enlace a la documentación publicada, para que cada paso diga dónde leer más.
class DocLink extends StatelessWidget {
  /// Crea el enlace.
  const DocLink({super.key, required this.label, required this.path});

  /// Texto visible.
  final String label;

  /// Ruta publicada por el portal (`/docs/…`).
  final String path;

  @override
  Widget build(BuildContext context) {
    return Align(
      alignment: Alignment.centerLeft,
      child: TextButton.icon(
        onPressed: () => openLink(path),
        icon: const Icon(Icons.menu_book_outlined, size: 18),
        label: Text(label),
      ),
    );
  }
}

String _messageFor(ConsoleApiException error) {
  // Un 404 sin cuerpo del contrato no viene de la API de la consola: la consola no está
  // activada en este entorno (sin clave de correo) y el borde no encuentra la ruta.
  if (error.status == 404 && error.code.isEmpty) {
    return 'El registro todavía no está activo en este entorno. Inténtalo más tarde.';
  }
  if (error.status == 0) {
    return 'No pudimos conectar con el servicio. Revisa tu conexión e inténtalo de nuevo.';
  }
  switch (error.code) {
    case 'UNAUTHENTICATED':
      return 'El código no es válido o ya caducó. Pide uno nuevo.';
    case 'RATE_LIMITED':
      return 'Demasiados intentos. Espera un momento y vuelve a intentarlo.';
    case 'VALIDATION_FAILED':
      switch (error.field) {
        case 'email':
          return 'Escribe un correo válido.';
        case 'code':
          return 'El código son solo números.';
        case 'accept_terms':
          return 'Para crear la cuenta tienes que aceptar las condiciones.';
        case 'name':
          return 'El nombre no es válido (1 a 80 caracteres).';
        case 'label':
          return 'La etiqueta no es válida (1 a 64 caracteres).';
        case 'scopes':
          return 'Elige al menos un permiso.';
      }
      return 'Revisa los datos e inténtalo de nuevo.';
    case 'PERMISSION_DENIED':
      return 'Tu rol en esta organización no permite esta acción.';
    case 'NOT_FOUND':
      return 'No se encontró. Puede que ya no exista.';
  }
  return 'No pudimos completar la acción. Inténtalo de nuevo en un momento.';
}

/// Correo → código → (condiciones si la cuenta es nueva). Registro e inicio de sesión son la misma ceremonia.
class SignInForm extends StatefulWidget {
  /// Crea el formulario.
  const SignInForm({super.key, required this.api, required this.onSignedIn});

  /// Cliente de la consola.
  final ConsoleApi api;

  /// Se llama con la sesión abierta.
  final ValueChanged<ConsoleSessionInfo> onSignedIn;

  @override
  State<SignInForm> createState() => _SignInFormState();
}

class _SignInFormState extends State<SignInForm> {
  final TextEditingController _email = TextEditingController();
  final TextEditingController _code = TextEditingController();
  bool _codeSent = false;
  bool _terms = false;
  bool _needsTerms = false;
  bool _busy = false;
  String? _error;

  @override
  void dispose() {
    _email.dispose();
    _code.dispose();
    super.dispose();
  }

  Future<void> _guard(Future<void> Function() action) async {
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      await action();
    } on ConsoleApiException catch (error) {
      if (!mounted) return;
      setState(() {
        _error = _messageFor(error);
        _needsTerms = _needsTerms || error.field == 'accept_terms';
      });
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _sendCode() => _guard(() async {
    await widget.api.requestCode(_email.text.trim());
    if (mounted) setState(() => _codeSent = true);
  });

  Future<void> _verify() => _guard(() async {
    final info = await widget.api.verify(
      _email.text.trim(),
      _code.text.trim(),
      acceptTerms: _terms,
    );
    widget.onSignedIn(info);
  });

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return ListView(
      shrinkWrap: true,
      children: [
        Text('Crea tu cuenta o entra', style: theme.textTheme.headlineSmall),
        const SizedBox(height: 8),
        const Text(
          'Sin contraseñas: te enviamos un código de un solo uso a tu correo. '
          'Si es tu primera vez, se crea la cuenta; si ya la tienes, entras. '
          'Necesitas la cuenta para generar las API keys con las que tu '
          'aplicación consume GeoStream.',
        ),
        const DocLink(
          label: 'Cómo se integra GeoStream (guía)',
          path: '/docs/15_INTEGRATION_GUIDE.html',
        ),
        const SizedBox(height: 16),
        TextField(
          controller: _email,
          enabled: !_codeSent,
          keyboardType: TextInputType.emailAddress,
          autofillHints: const [AutofillHints.email],
          decoration: const InputDecoration(
            labelText: 'Correo',
            border: OutlineInputBorder(),
          ),
          onSubmitted: (_) => _codeSent ? null : _sendCode(),
        ),
        if (_codeSent) ...[
          const SizedBox(height: 12),
          const Text(
            'Si el correo es válido, te enviamos un código. Caduca en 10 '
            'minutos. Revisa también la carpeta de spam.',
          ),
          const SizedBox(height: 12),
          TextField(
            controller: _code,
            keyboardType: TextInputType.number,
            inputFormatters: [FilteringTextInputFormatter.digitsOnly],
            autofillHints: const [AutofillHints.oneTimeCode],
            decoration: const InputDecoration(
              labelText: 'Código',
              border: OutlineInputBorder(),
            ),
            onSubmitted: (_) => _verify(),
          ),
          CheckboxListTile(
            contentPadding: EdgeInsets.zero,
            controlAffinity: ListTileControlAffinity.leading,
            value: _terms,
            onChanged: (value) => setState(() => _terms = value ?? false),
            title: Text(
              _needsTerms
                  ? 'Acepto las condiciones de uso y el tratamiento de mi correo para crear la cuenta (obligatorio).'
                  : 'Acepto las condiciones de uso (solo si es mi primera vez).',
            ),
          ),
        ],
        if (_error != null) ...[
          const SizedBox(height: 12),
          Text(_error!, style: TextStyle(color: theme.colorScheme.error)),
        ],
        const SizedBox(height: 16),
        FilledButton(
          onPressed: _busy ? null : (_codeSent ? _verify : _sendCode),
          child: Text(
            _busy
                ? 'Un momento…'
                : (_codeSent ? 'Entrar' : 'Enviarme un código'),
          ),
        ),
        if (_codeSent)
          TextButton(
            onPressed: _busy
                ? null
                : () => setState(() {
                    _codeSent = false;
                    _code.clear();
                    _error = null;
                  }),
            child: const Text('Usar otro correo o pedir otro código'),
          ),
      ],
    );
  }
}

/// Organizaciones de la cuenta y alta de nuevas.
class OrganizationsView extends StatefulWidget {
  /// Crea la vista.
  const OrganizationsView({
    super.key,
    required this.api,
    required this.onExpired,
  });

  /// Cliente de la consola.
  final ConsoleApi api;

  /// La sesión dejó de valer.
  final VoidCallback onExpired;

  @override
  State<OrganizationsView> createState() => _OrganizationsViewState();
}

class _OrganizationsViewState extends State<OrganizationsView> {
  List<Organization>? _organizations;
  String? _error;
  final TextEditingController _name = TextEditingController();

  @override
  void initState() {
    super.initState();
    _load();
  }

  @override
  void dispose() {
    _name.dispose();
    super.dispose();
  }

  Future<void> _load() async {
    try {
      final list = await widget.api.organizations();
      if (mounted) setState(() => _organizations = list);
    } on ConsoleApiException catch (error) {
      _fail(error);
    }
  }

  void _fail(ConsoleApiException error) {
    if (error.isUnauthenticated) {
      widget.onExpired();
    } else if (mounted) {
      setState(() => _error = _messageFor(error));
    }
  }

  Future<void> _create() async {
    final name = _name.text.trim();
    if (name.isEmpty) return;
    try {
      await widget.api.createOrganization(name);
      _name.clear();
      if (mounted) setState(() => _error = null);
      await _load();
    } on ConsoleApiException catch (error) {
      _fail(error);
    }
  }

  @override
  Widget build(BuildContext context) {
    final organizations = _organizations;
    if (organizations == null && _error == null) {
      return const Center(child: CircularProgressIndicator());
    }
    return ListView(
      children: [
        Text(
          'Organizaciones',
          style: Theme.of(context).textTheme.headlineSmall,
        ),
        const SizedBox(height: 8),
        if (organizations != null && organizations.isEmpty)
          const Text(
            'Aún no tienes ninguna. Una organización agrupa tus proyectos; '
            'crea la primera para empezar.',
          ),
        const DocLink(
          label: 'Modelo de organizaciones y proyectos',
          path: '/docs/15_INTEGRATION_GUIDE.html',
        ),
        for (final org in organizations ?? const <Organization>[])
          Card(
            child: ListTile(
              title: Text(org.name),
              subtitle: Text('Rol: ${org.role}'),
              trailing: const Icon(Icons.chevron_right),
              onTap: () => Navigator.of(context).push(
                MaterialPageRoute<void>(
                  builder: (_) => ProjectsView(
                    api: widget.api,
                    organization: org,
                    onExpired: widget.onExpired,
                  ),
                ),
              ),
            ),
          ),
        const SizedBox(height: 16),
        Row(
          children: [
            Expanded(
              child: TextField(
                controller: _name,
                decoration: const InputDecoration(
                  labelText: 'Nombre de la organización',
                  border: OutlineInputBorder(),
                ),
                onSubmitted: (_) => _create(),
              ),
            ),
            const SizedBox(width: 12),
            FilledButton(onPressed: _create, child: const Text('Crear')),
          ],
        ),
        if (_error != null) ...[
          const SizedBox(height: 12),
          Text(
            _error!,
            style: TextStyle(color: Theme.of(context).colorScheme.error),
          ),
        ],
      ],
    );
  }
}

/// Proyectos de una organización.
class ProjectsView extends StatefulWidget {
  /// Crea la vista.
  const ProjectsView({
    super.key,
    required this.api,
    required this.organization,
    required this.onExpired,
  });

  /// Cliente de la consola.
  final ConsoleApi api;

  /// Organización abierta.
  final Organization organization;

  /// La sesión dejó de valer.
  final VoidCallback onExpired;

  @override
  State<ProjectsView> createState() => _ProjectsViewState();
}

class _ProjectsViewState extends State<ProjectsView> {
  List<Project>? _projects;
  String? _error;
  final TextEditingController _name = TextEditingController();

  @override
  void initState() {
    super.initState();
    _load();
  }

  @override
  void dispose() {
    _name.dispose();
    super.dispose();
  }

  void _fail(ConsoleApiException error) {
    if (error.isUnauthenticated) {
      Navigator.of(context).popUntil((route) => route.isFirst);
      widget.onExpired();
    } else if (mounted) {
      setState(() => _error = _messageFor(error));
    }
  }

  Future<void> _load() async {
    try {
      final list = await widget.api.projects(widget.organization.id);
      if (mounted) setState(() => _projects = list);
    } on ConsoleApiException catch (error) {
      _fail(error);
    }
  }

  Future<void> _create() async {
    final name = _name.text.trim();
    if (name.isEmpty) return;
    try {
      await widget.api.createProject(widget.organization.id, name);
      _name.clear();
      if (mounted) setState(() => _error = null);
      await _load();
    } on ConsoleApiException catch (error) {
      _fail(error);
    }
  }

  @override
  Widget build(BuildContext context) {
    final projects = _projects;
    return Scaffold(
      appBar: AppBar(title: Text(widget.organization.name)),
      body: Center(
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 720),
          child: Padding(
            padding: const EdgeInsets.all(16),
            child: projects == null && _error == null
                ? const Center(child: CircularProgressIndicator())
                : ListView(
                    children: [
                      Text(
                        'Proyectos',
                        style: Theme.of(context).textTheme.headlineSmall,
                      ),
                      const SizedBox(height: 8),
                      const Text(
                        'Un proyecto es tu aplicación. Sus API keys son las '
                        'credenciales con las que tu servidor llama a GeoStream.',
                      ),
                      const DocLink(
                        label: 'Referencia de la API',
                        path: '/docs/api.html',
                      ),
                      const SizedBox(height: 8),
                      for (final project in projects ?? const <Project>[])
                        Card(
                          child: ListTile(
                            title: Text(project.name),
                            subtitle: Text(
                              project.status == 'ACTIVE'
                                  ? 'Activo'
                                  : 'Deshabilitado',
                            ),
                            trailing: const Icon(Icons.vpn_key_outlined),
                            onTap: () => Navigator.of(context).push(
                              MaterialPageRoute<void>(
                                builder: (_) => KeysView(
                                  api: widget.api,
                                  organization: widget.organization,
                                  project: project,
                                  onExpired: widget.onExpired,
                                ),
                              ),
                            ),
                          ),
                        ),
                      if (widget.organization.canManage) ...[
                        const SizedBox(height: 16),
                        Row(
                          children: [
                            Expanded(
                              child: TextField(
                                controller: _name,
                                decoration: const InputDecoration(
                                  labelText: 'Nombre del proyecto',
                                  border: OutlineInputBorder(),
                                ),
                                onSubmitted: (_) => _create(),
                              ),
                            ),
                            const SizedBox(width: 12),
                            FilledButton(
                              onPressed: _create,
                              child: const Text('Crear'),
                            ),
                          ],
                        ),
                      ],
                      if (_error != null) ...[
                        const SizedBox(height: 12),
                        Text(
                          _error!,
                          style: TextStyle(
                            color: Theme.of(context).colorScheme.error,
                          ),
                        ),
                      ],
                    ],
                  ),
          ),
        ),
      ),
    );
  }
}

/// API keys de un proyecto: crear (el secreto se muestra una vez) y revocar.
class KeysView extends StatefulWidget {
  /// Crea la vista.
  const KeysView({
    super.key,
    required this.api,
    required this.organization,
    required this.project,
    required this.onExpired,
  });

  /// Cliente de la consola.
  final ConsoleApi api;

  /// Organización del proyecto.
  final Organization organization;

  /// Proyecto abierto.
  final Project project;

  /// La sesión dejó de valer.
  final VoidCallback onExpired;

  @override
  State<KeysView> createState() => _KeysViewState();
}

class _KeysViewState extends State<KeysView> {
  List<ApiKeyInfo>? _keys;
  String? _error;
  final TextEditingController _label = TextEditingController();
  final Set<String> _scopes = {};

  @override
  void initState() {
    super.initState();
    _load();
  }

  @override
  void dispose() {
    _label.dispose();
    super.dispose();
  }

  void _fail(ConsoleApiException error) {
    if (error.isUnauthenticated) {
      Navigator.of(context).popUntil((route) => route.isFirst);
      widget.onExpired();
    } else if (mounted) {
      setState(() => _error = _messageFor(error));
    }
  }

  Future<void> _load() async {
    try {
      final list = await widget.api.keys(
        widget.organization.id,
        widget.project.id,
      );
      if (mounted) setState(() => _keys = list);
    } on ConsoleApiException catch (error) {
      _fail(error);
    }
  }

  Future<void> _create() async {
    try {
      final created = await widget.api.createKey(
        widget.organization.id,
        widget.project.id,
        label: _label.text.trim(),
        scopes: _scopes.toList()..sort(),
      );
      _label.clear();
      if (mounted) {
        setState(() {
          _scopes.clear();
          _error = null;
        });
        await showDialog<void>(
          context: context,
          barrierDismissible: false,
          builder: (_) => SecretDialog(secret: created.secret),
        );
      }
      await _load();
    } on ConsoleApiException catch (error) {
      _fail(error);
    }
  }

  Future<void> _revoke(ApiKeyInfo key) async {
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('¿Revocar esta API key?'),
        content: Text(
          '«${key.label}» dejará de funcionar de inmediato. No se puede deshacer.',
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context, false),
            child: const Text('Cancelar'),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(context, true),
            child: const Text('Revocar'),
          ),
        ],
      ),
    );
    if (confirmed != true) return;
    try {
      await widget.api.revokeKey(
        widget.organization.id,
        widget.project.id,
        key.id,
      );
      await _load();
    } on ConsoleApiException catch (error) {
      _fail(error);
    }
  }

  @override
  Widget build(BuildContext context) {
    final keys = _keys;
    final theme = Theme.of(context);
    final canManage = widget.organization.canManage;
    return Scaffold(
      appBar: AppBar(title: Text(widget.project.name)),
      body: Center(
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 720),
          child: Padding(
            padding: const EdgeInsets.all(16),
            child: keys == null && _error == null
                ? const Center(child: CircularProgressIndicator())
                : ListView(
                    children: [
                      Text('API keys', style: theme.textTheme.headlineSmall),
                      const DocLink(
                        label: 'Cómo usar tu API key: emitir tokens y conectar',
                        path: '/docs/15_INTEGRATION_GUIDE.html',
                      ),
                      const SizedBox(height: 8),
                      for (final key in keys ?? const <ApiKeyInfo>[])
                        Card(
                          child: ListTile(
                            title: Text(
                              key.label,
                              style: key.revoked
                                  ? const TextStyle(
                                      decoration: TextDecoration.lineThrough,
                                    )
                                  : null,
                            ),
                            subtitle: Text(
                              '${key.prefix}… · ${key.scopes.join(', ')}'
                              '${key.revoked ? ' · revocada' : ''}',
                            ),
                            trailing: canManage && !key.revoked
                                ? IconButton(
                                    tooltip: 'Revocar',
                                    icon: const Icon(Icons.block),
                                    onPressed: () => _revoke(key),
                                  )
                                : null,
                          ),
                        ),
                      if (canManage && widget.project.status == 'ACTIVE') ...[
                        const SizedBox(height: 16),
                        Text(
                          'Nueva API key',
                          style: theme.textTheme.titleMedium,
                        ),
                        const SizedBox(height: 8),
                        TextField(
                          controller: _label,
                          decoration: const InputDecoration(
                            labelText:
                                'Etiqueta (por ejemplo, backend de producción)',
                            border: OutlineInputBorder(),
                          ),
                        ),
                        const SizedBox(height: 8),
                        for (final entry in keyScopes.entries)
                          CheckboxListTile(
                            contentPadding: EdgeInsets.zero,
                            controlAffinity: ListTileControlAffinity.leading,
                            value: _scopes.contains(entry.key),
                            onChanged: (on) => setState(() {
                              if (on ?? false) {
                                _scopes.add(entry.key);
                              } else {
                                _scopes.remove(entry.key);
                              }
                            }),
                            title: Text(entry.key),
                            subtitle: Text(entry.value),
                          ),
                        const SizedBox(height: 8),
                        FilledButton(
                          onPressed: _create,
                          child: const Text('Generar API key'),
                        ),
                      ],
                      if (_error != null) ...[
                        const SizedBox(height: 12),
                        Text(
                          _error!,
                          style: TextStyle(color: theme.colorScheme.error),
                        ),
                      ],
                    ],
                  ),
          ),
        ),
      ),
    );
  }
}

/// Muestra el secreto una sola vez. Cerrar el diálogo lo borra de la memoria de la pantalla.
class SecretDialog extends StatelessWidget {
  /// Crea el diálogo.
  const SecretDialog({super.key, required this.secret});

  /// La API key en claro.
  final String secret;

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      title: const Text('Copia tu API key ahora'),
      content: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          const Text(
            'Es la única vez que la verás. No la guardamos: si la pierdes, '
            'tendrás que generar otra.',
          ),
          const SizedBox(height: 12),
          SelectableText(
            secret,
            style: const TextStyle(fontFamily: 'monospace'),
          ),
        ],
      ),
      actions: [
        TextButton.icon(
          onPressed: () => Clipboard.setData(ClipboardData(text: secret)),
          icon: const Icon(Icons.copy),
          label: const Text('Copiar'),
        ),
        FilledButton(
          onPressed: () => Navigator.pop(context),
          child: const Text('Ya la guardé'),
        ),
      ],
    );
  }
}
