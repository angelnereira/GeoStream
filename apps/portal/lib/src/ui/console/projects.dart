import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../../console/console_api.dart';
import '../../platform/link_opener.dart';
import 'messages.dart';

/// URL pública de la API de GeoStream: la que usa el backend del integrador con su API key.
const String kApiBaseUrl = 'https://geostream.gravitalcloud.com';

/// Scopes que puede llevar una API key de aplicación (`docs/06` §3.4), con su explicación.
const Map<String, String> keyScopes = {
  'sessions:read': 'Leer sesiones',
  'sessions:write': 'Crear y cerrar sesiones',
  'members:write': 'Gestionar miembros',
  'invitations:write': 'Crear invitaciones',
  'tokens:mint': 'Emitir tokens para dispositivos',
  'stream:connect': 'Conectarse al stream en tiempo real',
  'locations:read': 'Leer ubicaciones (dato sensible)',
  'subjects:manage': 'Gestionar sujetos',
};

/// Conjuntos de permisos habituales, para no marcarlos uno a uno.
const Map<String, List<String>> keyPresets = {
  'Entregas completas': [
    'tokens:mint',
    'sessions:write',
    'sessions:read',
    'locations:read',
  ],
  'Solo lectura': ['sessions:read', 'locations:read'],
  'Solo emitir tokens': ['tokens:mint'],
};

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

/// Contenedor de una página de contenido: centrada, con ancho legible y desplazable.
class PageBody extends StatelessWidget {
  /// Crea el contenedor.
  const PageBody({super.key, required this.children});

  /// Contenido.
  final List<Widget> children;

  @override
  Widget build(BuildContext context) {
    return Align(
      alignment: Alignment.topCenter,
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 760),
        child: ListView(padding: const EdgeInsets.all(16), children: children),
      ),
    );
  }
}

mixin ConsoleErrors<T extends StatefulWidget> on State<T> {
  /// Último error mostrado.
  String? error;

  /// La sesión caducó: hay que volver a entrar.
  VoidCallback get onExpired;

  /// Trata un error: sesión caducada o mensaje en pantalla.
  void fail(ConsoleApiException e) {
    if (e.isUnauthenticated) {
      Navigator.of(context).popUntil((route) => route.isFirst);
      onExpired();
    } else if (mounted) {
      setState(() => error = messageFor(e));
    }
  }

  Widget errorText(BuildContext context) => error == null
      ? const SizedBox.shrink()
      : Padding(
          padding: const EdgeInsets.only(top: 12),
          child: Text(
            error!,
            style: TextStyle(color: Theme.of(context).colorScheme.error),
          ),
        );
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

class _OrganizationsViewState extends State<OrganizationsView>
    with ConsoleErrors<OrganizationsView> {
  List<Organization>? _organizations;
  final TextEditingController _name = TextEditingController();

  @override
  VoidCallback get onExpired => widget.onExpired;

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
    } on ConsoleApiException catch (e) {
      fail(e);
    }
  }

  Future<void> _create() async {
    final name = _name.text.trim();
    if (name.isEmpty) return;
    try {
      await widget.api.createOrganization(name);
      _name.clear();
      if (mounted) setState(() => error = null);
      await _load();
    } on ConsoleApiException catch (e) {
      fail(e);
    }
  }

  @override
  Widget build(BuildContext context) {
    final organizations = _organizations;
    if (organizations == null && error == null) {
      return const Center(child: CircularProgressIndicator());
    }
    return PageBody(
      children: [
        Text('Proyectos', style: Theme.of(context).textTheme.headlineSmall),
        const SizedBox(height: 4),
        const Text(
          'Una organización agrupa tus proyectos; cada proyecto es una '
          'aplicación con sus propias API keys.',
        ),
        const SizedBox(height: 12),
        if (organizations != null && organizations.isEmpty)
          const Card(
            child: Padding(
              padding: EdgeInsets.all(16),
              child: Text(
                'Aún no tienes ninguna organización. Crea la primera para '
                'empezar: después podrás crear un proyecto y generar su API key.',
              ),
            ),
          ),
        for (final org in organizations ?? const <Organization>[])
          Card(
            child: ListTile(
              leading: const Icon(Icons.apartment_outlined),
              title: Text(org.name),
              subtitle: Text('Tu rol: ${_roleName(org.role)}'),
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
        errorText(context),
      ],
    );
  }
}

String _roleName(String role) => switch (role) {
  'OWNER' => 'propietario',
  'ADMIN' => 'administrador',
  _ => 'solo lectura',
};

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

class _ProjectsViewState extends State<ProjectsView>
    with ConsoleErrors<ProjectsView> {
  List<Project>? _projects;
  final TextEditingController _name = TextEditingController();

  @override
  VoidCallback get onExpired => widget.onExpired;

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
      final list = await widget.api.projects(widget.organization.id);
      if (mounted) setState(() => _projects = list);
    } on ConsoleApiException catch (e) {
      fail(e);
    }
  }

  Future<void> _create() async {
    final name = _name.text.trim();
    if (name.isEmpty) return;
    try {
      await widget.api.createProject(widget.organization.id, name);
      _name.clear();
      if (mounted) setState(() => error = null);
      await _load();
    } on ConsoleApiException catch (e) {
      fail(e);
    }
  }

  @override
  Widget build(BuildContext context) {
    final projects = _projects;
    return Scaffold(
      appBar: AppBar(title: Text(widget.organization.name)),
      body: projects == null && error == null
          ? const Center(child: CircularProgressIndicator())
          : PageBody(
              children: [
                Text(
                  'Proyectos',
                  style: Theme.of(context).textTheme.headlineSmall,
                ),
                const SizedBox(height: 4),
                const Text(
                  'Abre un proyecto para gestionar sus API keys y su configuración.',
                ),
                const DocLink(
                  label: 'Referencia de la API',
                  path: '/docs/api.html',
                ),
                if (projects != null && projects.isEmpty)
                  const Card(
                    child: Padding(
                      padding: EdgeInsets.all(16),
                      child: Text('Esta organización aún no tiene proyectos.'),
                    ),
                  ),
                for (final project in projects ?? const <Project>[])
                  Card(
                    child: ListTile(
                      leading: Icon(
                        project.active
                            ? Icons.check_circle_outline
                            : Icons.pause_circle_outline,
                      ),
                      title: Text(project.name),
                      subtitle: Text(
                        project.active ? 'Activo' : 'Deshabilitado',
                      ),
                      trailing: const Icon(Icons.chevron_right),
                      onTap: () async {
                        await Navigator.of(context).push(
                          MaterialPageRoute<void>(
                            builder: (_) => ProjectDetailView(
                              api: widget.api,
                              organization: widget.organization,
                              project: project,
                              onExpired: widget.onExpired,
                            ),
                          ),
                        );
                        await _load();
                      },
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
                            labelText: 'Nombre del proyecto nuevo',
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
                errorText(context),
              ],
            ),
    );
  }
}

/// Un proyecto: sus API keys y su configuración, en dos pestañas.
class ProjectDetailView extends StatefulWidget {
  /// Crea la vista.
  const ProjectDetailView({
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
  State<ProjectDetailView> createState() => _ProjectDetailViewState();
}

class _ProjectDetailViewState extends State<ProjectDetailView> {
  late Project _project = widget.project;

  @override
  Widget build(BuildContext context) {
    return DefaultTabController(
      length: 2,
      child: Scaffold(
        appBar: AppBar(
          title: Text(_project.name),
          bottom: const TabBar(
            tabs: [
              Tab(text: 'API keys'),
              Tab(text: 'Configuración'),
            ],
          ),
        ),
        body: TabBarView(
          children: [
            KeysTab(
              api: widget.api,
              organization: widget.organization,
              project: _project,
              onExpired: widget.onExpired,
            ),
            SettingsTab(
              api: widget.api,
              organization: widget.organization,
              project: _project,
              onExpired: widget.onExpired,
              onChanged: (p) => setState(() => _project = p),
            ),
          ],
        ),
      ),
    );
  }
}

/// API keys de un proyecto: crear varias, con permisos distintos, y revocarlas.
class KeysTab extends StatefulWidget {
  /// Crea la pestaña.
  const KeysTab({
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
  State<KeysTab> createState() => _KeysTabState();
}

class _KeysTabState extends State<KeysTab> with ConsoleErrors<KeysTab> {
  List<ApiKeyInfo>? _keys;

  @override
  VoidCallback get onExpired => widget.onExpired;

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    try {
      final list = await widget.api.keys(
        widget.organization.id,
        widget.project.id,
      );
      if (mounted) setState(() => _keys = list);
    } on ConsoleApiException catch (e) {
      fail(e);
    }
  }

  Future<void> _newKey() async {
    final request = await showDialog<NewKeyRequest>(
      context: context,
      builder: (_) => const NewKeyDialog(),
    );
    if (request == null || !mounted) return;
    try {
      final created = await widget.api.createKey(
        widget.organization.id,
        widget.project.id,
        label: request.label,
        scopes: request.scopes,
        expiresInDays: request.expiresInDays,
      );
      if (mounted) {
        setState(() => error = null);
        await showDialog<void>(
          context: context,
          barrierDismissible: false,
          builder: (_) => SecretDialog(secret: created.secret),
        );
      }
      await _load();
    } on ConsoleApiException catch (e) {
      fail(e);
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
    } on ConsoleApiException catch (e) {
      fail(e);
    }
  }

  @override
  Widget build(BuildContext context) {
    final keys = _keys;
    if (keys == null && error == null) {
      return const Center(child: CircularProgressIndicator());
    }
    final canManage = widget.organization.canManage && widget.project.active;
    final active = (keys ?? const <ApiKeyInfo>[])
        .where((k) => !k.revoked)
        .toList();
    final revoked = (keys ?? const <ApiKeyInfo>[])
        .where((k) => k.revoked)
        .toList();
    return PageBody(
      children: [
        Row(
          children: [
            Expanded(
              child: Text(
                'API keys',
                style: Theme.of(context).textTheme.headlineSmall,
              ),
            ),
            if (canManage)
              FilledButton.icon(
                onPressed: _newKey,
                icon: const Icon(Icons.add),
                label: const Text('Nueva API key'),
              ),
          ],
        ),
        const SizedBox(height: 4),
        const Text(
          'Puedes tener varias: una por servicio o entorno, cada una con solo los '
          'permisos que necesita, y revocar una sin afectar a las demás. La clave '
          'completa se muestra una sola vez al crearla.',
        ),
        const DocLink(
          label: 'Cómo usar tu API key: emitir tokens y conectar',
          path: '/docs/15_INTEGRATION_GUIDE.html',
        ),
        if (!widget.project.active)
          const Card(
            child: Padding(
              padding: EdgeInsets.all(16),
              child: Text(
                'Este proyecto está deshabilitado: no se pueden crear claves. '
                'Actívalo en «Configuración».',
              ),
            ),
          ),
        if (keys != null && keys.isEmpty)
          const Card(
            child: Padding(
              padding: EdgeInsets.all(16),
              child: Text('Este proyecto aún no tiene API keys.'),
            ),
          ),
        for (final key in active)
          _KeyTile(info: key, onRevoke: canManage ? _revoke : null),
        if (revoked.isNotEmpty) ...[
          const SizedBox(height: 16),
          Text('Revocadas', style: Theme.of(context).textTheme.titleSmall),
          for (final key in revoked) _KeyTile(info: key, onRevoke: null),
        ],
        errorText(context),
      ],
    );
  }
}

class _KeyTile extends StatelessWidget {
  const _KeyTile({required this.info, required this.onRevoke});

  final ApiKeyInfo info;
  final Future<void> Function(ApiKeyInfo key)? onRevoke;

  @override
  Widget build(BuildContext context) {
    final key = info;
    final created = key.createdAt?.split('T').first;
    return Card(
      child: ListTile(
        leading: Icon(key.revoked ? Icons.block : Icons.vpn_key_outlined),
        title: Text(
          key.label,
          style: key.revoked
              ? const TextStyle(decoration: TextDecoration.lineThrough)
              : null,
        ),
        subtitle: Text(
          '${key.prefix}…'
          '${created == null ? '' : ' · creada $created'}'
          '${key.expiresAt == null ? '' : ' · caduca ${key.expiresAt!.split('T').first}'}'
          '\n${key.scopes.join(', ')}',
        ),
        isThreeLine: true,
        trailing: onRevoke == null
            ? null
            : IconButton(
                tooltip: 'Revocar',
                icon: const Icon(Icons.block),
                onPressed: () => onRevoke!(key),
              ),
      ),
    );
  }
}

/// Lo que pide el diálogo de nueva API key.
class NewKeyRequest {
  /// Crea la petición.
  const NewKeyRequest({
    required this.label,
    required this.scopes,
    this.expiresInDays,
  });

  /// Etiqueta.
  final String label;

  /// Permisos.
  final List<String> scopes;

  /// Días hasta que caduque, o `null` si no caduca.
  final int? expiresInDays;
}

/// Diálogo para crear una API key: etiqueta, permisos (con conjuntos habituales) y caducidad.
class NewKeyDialog extends StatefulWidget {
  /// Crea el diálogo.
  const NewKeyDialog({super.key});

  @override
  State<NewKeyDialog> createState() => _NewKeyDialogState();
}

class _NewKeyDialogState extends State<NewKeyDialog> {
  final TextEditingController _label = TextEditingController();
  final Set<String> _scopes = {};
  int? _days;
  String? _problem;

  @override
  void dispose() {
    _label.dispose();
    super.dispose();
  }

  void _submit() {
    final label = _label.text.trim();
    if (label.isEmpty || label.length > 64) {
      setState(() => _problem = 'Ponle una etiqueta de 1 a 64 caracteres.');
      return;
    }
    if (_scopes.isEmpty) {
      setState(() => _problem = 'Elige al menos un permiso.');
      return;
    }
    Navigator.pop(
      context,
      NewKeyRequest(
        label: label,
        scopes: (_scopes.toList()..sort()),
        expiresInDays: _days,
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      title: const Text('Nueva API key'),
      content: SizedBox(
        width: 460,
        child: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              TextField(
                controller: _label,
                autofocus: true,
                decoration: const InputDecoration(
                  labelText: 'Etiqueta (por ejemplo, backend de producción)',
                  border: OutlineInputBorder(),
                ),
              ),
              const SizedBox(height: 16),
              Text('Permisos', style: Theme.of(context).textTheme.titleSmall),
              const SizedBox(height: 4),
              Wrap(
                spacing: 8,
                children: [
                  for (final preset in keyPresets.entries)
                    ActionChip(
                      label: Text(preset.key),
                      onPressed: () => setState(() {
                        _scopes
                          ..clear()
                          ..addAll(preset.value);
                      }),
                    ),
                ],
              ),
              for (final entry in keyScopes.entries)
                CheckboxListTile(
                  dense: true,
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
              DropdownButtonFormField<int?>(
                initialValue: _days,
                decoration: const InputDecoration(
                  labelText: 'Caducidad',
                  border: OutlineInputBorder(),
                ),
                items: const [
                  DropdownMenuItem(value: null, child: Text('No caduca')),
                  DropdownMenuItem(value: 30, child: Text('A los 30 días')),
                  DropdownMenuItem(value: 90, child: Text('A los 90 días')),
                  DropdownMenuItem(value: 365, child: Text('Al año')),
                ],
                onChanged: (v) => setState(() => _days = v),
              ),
              if (_problem != null) ...[
                const SizedBox(height: 12),
                Text(
                  _problem!,
                  style: TextStyle(color: Theme.of(context).colorScheme.error),
                ),
              ],
            ],
          ),
        ),
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.pop(context),
          child: const Text('Cancelar'),
        ),
        FilledButton(onPressed: _submit, child: const Text('Generar')),
      ],
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
            'tendrás que generar otra. Guárdala en el gestor de secretos de tu '
            'servidor; nunca la pongas en una app ni en una página web.',
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

/// Configuración de un proyecto: nombre, estado, orígenes permitidos y datos de conexión.
class SettingsTab extends StatefulWidget {
  /// Crea la pestaña.
  const SettingsTab({
    super.key,
    required this.api,
    required this.organization,
    required this.project,
    required this.onExpired,
    required this.onChanged,
  });

  /// Cliente de la consola.
  final ConsoleApi api;

  /// Organización del proyecto.
  final Organization organization;

  /// Proyecto abierto.
  final Project project;

  /// La sesión dejó de valer.
  final VoidCallback onExpired;

  /// El proyecto cambió: devuelve su nuevo estado.
  final ValueChanged<Project> onChanged;

  @override
  State<SettingsTab> createState() => _SettingsTabState();
}

class _SettingsTabState extends State<SettingsTab>
    with ConsoleErrors<SettingsTab> {
  late final TextEditingController _name = TextEditingController(
    text: widget.project.name,
  );
  late final TextEditingController _origins = TextEditingController(
    text: widget.project.allowedOrigins.join('\n'),
  );
  late Project _project = widget.project;
  String? _saved;

  @override
  VoidCallback get onExpired => widget.onExpired;

  @override
  void dispose() {
    _name.dispose();
    _origins.dispose();
    super.dispose();
  }

  Future<void> _save({
    String? name,
    bool? active,
    List<String>? origins,
  }) async {
    try {
      final updated = await widget.api.updateProject(
        widget.organization.id,
        _project.id,
        name: name,
        active: active,
        allowedOrigins: origins,
      );
      if (!mounted) return;
      setState(() {
        _project = updated;
        error = null;
        _saved = 'Cambios guardados.';
      });
      widget.onChanged(updated);
    } on ConsoleApiException catch (e) {
      setState(() => _saved = null);
      fail(e);
    }
  }

  Future<void> _toggleActive(bool active) async {
    if (!active) {
      final ok = await showDialog<bool>(
        context: context,
        builder: (context) => AlertDialog(
          title: const Text('¿Deshabilitar el proyecto?'),
          content: const Text(
            'Sus API keys y los tokens que emitió dejarán de funcionar y se '
            'cerrarán sus conexiones en vivo. Puedes volver a activarlo.',
          ),
          actions: [
            TextButton(
              onPressed: () => Navigator.pop(context, false),
              child: const Text('Cancelar'),
            ),
            FilledButton(
              onPressed: () => Navigator.pop(context, true),
              child: const Text('Deshabilitar'),
            ),
          ],
        ),
      );
      if (ok != true) return;
    }
    await _save(active: active);
  }

  List<String> _parseOrigins() => _origins.text
      .split(RegExp(r'[\n,]'))
      .map((o) => o.trim())
      .where((o) => o.isNotEmpty)
      .toList();

  @override
  Widget build(BuildContext context) {
    final canManage = widget.organization.canManage;
    return PageBody(
      children: [
        Text('Configuración', style: Theme.of(context).textTheme.headlineSmall),
        const SizedBox(height: 12),
        Card(
          child: Padding(
            padding: const EdgeInsets.all(16),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text('Conexión', style: Theme.of(context).textTheme.titleSmall),
                const SizedBox(height: 8),
                _CopyRow(label: 'URL de la API', value: kApiBaseUrl),
                _CopyRow(label: 'ID del proyecto', value: _project.id),
                const SizedBox(height: 4),
                const Text(
                  'Tu servidor llama a la API con una de las API keys de este '
                  'proyecto en la cabecera Authorization: Bearer.',
                ),
              ],
            ),
          ),
        ),
        Card(
          child: Padding(
            padding: const EdgeInsets.all(16),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text('Nombre', style: Theme.of(context).textTheme.titleSmall),
                const SizedBox(height: 8),
                TextField(
                  controller: _name,
                  enabled: canManage,
                  decoration: const InputDecoration(
                    border: OutlineInputBorder(),
                  ),
                ),
                if (canManage) ...[
                  const SizedBox(height: 8),
                  FilledButton(
                    onPressed: () => _save(name: _name.text.trim()),
                    child: const Text('Guardar nombre'),
                  ),
                ],
              ],
            ),
          ),
        ),
        Card(
          child: Padding(
            padding: const EdgeInsets.all(16),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  'Orígenes web permitidos',
                  style: Theme.of(context).textTheme.titleSmall,
                ),
                const SizedBox(height: 4),
                const Text(
                  'Uno por línea, como https://app.ejemplo.com, sin ruta. Solo '
                  'hacen falta si una página web llamará a GeoStream.',
                ),
                const SizedBox(height: 8),
                TextField(
                  controller: _origins,
                  enabled: canManage,
                  minLines: 2,
                  maxLines: 6,
                  decoration: const InputDecoration(
                    border: OutlineInputBorder(),
                  ),
                ),
                if (canManage) ...[
                  const SizedBox(height: 8),
                  FilledButton(
                    onPressed: () => _save(origins: _parseOrigins()),
                    child: const Text('Guardar orígenes'),
                  ),
                ],
              ],
            ),
          ),
        ),
        Card(
          child: SwitchListTile(
            title: const Text('Proyecto activo'),
            subtitle: Text(
              _project.active
                  ? 'Sus API keys funcionan.'
                  : 'Deshabilitado: ninguna de sus API keys funciona.',
            ),
            value: _project.active,
            onChanged: canManage ? _toggleActive : null,
          ),
        ),
        if (!canManage)
          const Padding(
            padding: EdgeInsets.only(top: 8),
            child: Text('Tu rol solo permite ver esta configuración.'),
          ),
        if (_saved != null)
          Padding(padding: const EdgeInsets.only(top: 8), child: Text(_saved!)),
        errorText(context),
      ],
    );
  }
}

class _CopyRow extends StatelessWidget {
  const _CopyRow({required this.label, required this.value});

  final String label;
  final String value;

  @override
  Widget build(BuildContext context) {
    return Row(
      children: [
        SizedBox(width: 120, child: Text(label)),
        Expanded(
          child: SelectableText(
            value,
            style: const TextStyle(fontFamily: 'monospace'),
          ),
        ),
        IconButton(
          tooltip: 'Copiar $label',
          icon: const Icon(Icons.copy, size: 18),
          onPressed: () => Clipboard.setData(ClipboardData(text: value)),
        ),
      ],
    );
  }
}
