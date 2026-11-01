import 'package:flutter/material.dart';

import '../platform/link_opener.dart';
import '../status/status_client.dart';
import 'status_card.dart';

/// Ancho máximo del contenido: el texto largo se lee mal en pantallas anchas.
const double _maxContentWidth = 1040;

/// Documentos publicados. Las rutas las genera `tool/build_docs.sh` a partir del manifiesto.
const List<({String title, String detail, String path})> _publishedDocs = [
  (
    title: 'Referencia de la API REST',
    detail: 'Contrato OpenAPI 3.1 (borrador hasta el hito M2: puede cambiar).',
    path: '/docs/api.html',
  ),
  (
    title: 'Guía de integración',
    detail: 'Cómo consumir GeoStream desde tu backend, móvil o dispositivo.',
    path: '/docs/15_INTEGRATION_GUIDE.html',
  ),
  (
    title: 'Protocolo de stream',
    detail: 'WebSocket: handshake, mensajes, cierres y reconexión.',
    path: '/docs/08_STREAM_PROTOCOL.html',
  ),
  (
    title: 'Validación y errores',
    detail: 'Reglas por campo y catálogo de códigos de error.',
    path: '/docs/09_VALIDATION_ERRORS.html',
  ),
  (
    title: 'Contratos Protobuf',
    detail: 'Los archivos .proto del stream y de los modelos.',
    path: '/contracts/',
  ),
];

/// Capacidades y su estado real. Solo «hoy» si está implementado y probado
/// (`docs/PROGRESS.md`); lo demás se declara como en camino.
const List<({String name, bool today, String note})> _capabilities = [
  (
    name: 'Sesiones, miembros e invitaciones',
    today: true,
    note: 'Con roles y capacidades por miembro.',
  ),
  (
    name: 'Compartir ubicación en tiempo real',
    today: true,
    note: 'Apagado por defecto: solo el propio usuario lo activa.',
  ),
  (
    name: 'Stream en vivo (WebSocket)',
    today: true,
    note: 'Protobuf binario y JSON, con renovación de credenciales.',
  ),
  (
    name: 'Instantánea REST y publicación por lotes HTTP',
    today: true,
    note: 'Para backends y dispositivos sin conexión persistente.',
  ),
  (
    name: 'Varias instancias detrás de un balanceador',
    today: true,
    note: 'Estado y distribución compartidos.',
  ),
  (
    name: 'Consola con registro e inicio de sesión',
    today: false,
    note: 'Depende de que Gravital ofrezca OpenID Connect.',
  ),
  (
    name: 'Mapa en vivo y demo con datos simulados',
    today: false,
    note: 'Con mapa propio basado en OpenStreetMap.',
  ),
  (
    name: 'Geocercas en el servidor y webhooks',
    today: false,
    note: 'Hoy las geocercas se evalúan en el cliente.',
  ),
];

/// Página única: presentación, estado, documentación y cómo empezar.
class LandingPage extends StatelessWidget {
  /// Crea la página.
  const LandingPage({super.key, required this.status});

  /// Origen del estado en vivo.
  final StatusClient status;

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      body: SelectionArea(
        child: SingleChildScrollView(
          child: Center(
            child: ConstrainedBox(
              constraints: const BoxConstraints(maxWidth: _maxContentWidth),
              child: Padding(
                padding: const EdgeInsets.symmetric(horizontal: 16),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    const _Header(),
                    const SizedBox(height: 32),
                    const _Hero(),
                    const SizedBox(height: 32),
                    _Section(
                      title: 'Estado del servicio',
                      child: StatusCard(client: status),
                    ),
                    const _Section(
                      title: 'Qué hay hoy y qué viene',
                      child: _CapabilityList(),
                    ),
                    const _Section(
                      title: 'Documentación y contratos',
                      child: _DocsGrid(),
                    ),
                    const _Section(title: 'Cómo empezar', child: _Steps()),
                    const SizedBox(height: 48),
                    const _Footer(),
                    const SizedBox(height: 24),
                  ],
                ),
              ),
            ),
          ),
        ),
      ),
    );
  }
}

class _Header extends StatelessWidget {
  const _Header();

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Padding(
      padding: const EdgeInsets.only(top: 20),
      child: Row(
        children: [
          Icon(Icons.my_location, color: theme.colorScheme.primary),
          const SizedBox(width: 8),
          Text('GeoStream', style: theme.textTheme.titleLarge),
        ],
      ),
    );
  }
}

class _Hero extends StatelessWidget {
  const _Hero();

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(
          'Ubicación en tiempo real, como servicio.',
          style: theme.textTheme.displaySmall,
        ),
        const SizedBox(height: 12),
        Text(
          'Integra seguimiento en vivo en tu producto con una API y un '
          'protocolo documentados. La privacidad viene por defecto: nadie '
          'comparte su ubicación sin activarlo, y cada proyecto está '
          'aislado de los demás.',
          style: theme.textTheme.bodyLarge,
        ),
      ],
    );
  }
}

class _Section extends StatelessWidget {
  const _Section({required this.title, required this.child});

  final String title;
  final Widget child;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.only(top: 32),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Semantics(
            header: true,
            child: Text(title, style: Theme.of(context).textTheme.headlineSmall),
          ),
          const SizedBox(height: 12),
          child,
        ],
      ),
    );
  }
}

class _CapabilityList extends StatelessWidget {
  const _CapabilityList();

  @override
  Widget build(BuildContext context) {
    return Card(
      child: Column(
        children: [
          for (final capability in _capabilities)
            ListTile(
              leading: Icon(
                capability.today ? Icons.check_circle : Icons.schedule,
                semanticLabel: capability.today ? 'Disponible hoy' : 'En camino',
              ),
              title: Text(capability.name),
              subtitle: Text(capability.note),
              trailing: Text(capability.today ? 'Hoy' : 'En camino'),
            ),
        ],
      ),
    );
  }
}

class _DocsGrid extends StatelessWidget {
  const _DocsGrid();

  @override
  Widget build(BuildContext context) {
    return Wrap(
      spacing: 12,
      runSpacing: 12,
      children: [
        for (final doc in _publishedDocs)
          SizedBox(
            width: 320,
            child: Card(
              child: InkWell(
                onTap: () => openLink(doc.path),
                child: Padding(
                  padding: const EdgeInsets.all(16),
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        doc.title,
                        style: Theme.of(context).textTheme.titleMedium,
                      ),
                      const SizedBox(height: 4),
                      Text(doc.detail),
                    ],
                  ),
                ),
              ),
            ),
          ),
      ],
    );
  }
}

class _Steps extends StatelessWidget {
  const _Steps();

  @override
  Widget build(BuildContext context) {
    const steps = [
      ('1', 'Pide acceso', 'Hoy el alta de cada proyecto la hace el operador y te entrega una API key. El registro por autoservicio llega con la consola.'),
      ('2', 'Obtén tokens', 'Tu backend usa la API key para emitir un token a cada dispositivo de tus usuarios. Nunca pongas la API key en una app.'),
      ('3', 'Conecta', 'Abre el stream con el token y publica o recibe ubicaciones. La guía de integración tiene ejemplos paso a paso.'),
    ];
    return Column(
      children: [
        for (final (number, title, detail) in steps)
          Card(
            child: ListTile(
              leading: CircleAvatar(child: Text(number)),
              title: Text(title),
              subtitle: Text(detail),
            ),
          ),
      ],
    );
  }
}

class _Footer extends StatelessWidget {
  const _Footer();

  @override
  Widget build(BuildContext context) {
    return Text(
      'Los datos de mapas, cuando se muestran, son © OpenStreetMap '
      'contributors (ODbL). Las cifras de rendimiento de la documentación son '
      'objetivos, no mediciones, salvo que se indique lo contrario.',
      style: Theme.of(context).textTheme.bodySmall,
    );
  }
}
