import 'package:flutter/material.dart';
import 'package:http/http.dart' as http;
import 'package:maplibre_gl/maplibre_gl.dart';

import 'src/api/api_client.dart';
import 'src/console/console_api.dart';
import 'src/status/status_client.dart';
import 'src/ui/console_page.dart';
import 'src/ui/developer_console.dart';
import 'src/ui/landing_page.dart';

void main() {
  // MapLibre se sirve desde el propio portal: la CSP no admite CDN (ADR-0034, 10 T-38).
  MapLibreMap.webLibrarySource = MapLibreJsSource.urls(
    scriptUrl: Uri.base
        .resolve('/vendor/maplibre-gl/maplibre-gl.mjs')
        .toString(),
    styleUrl: Uri.base
        .resolve('/vendor/maplibre-gl/maplibre-gl.css')
        .toString(),
  );
  runApp(
    PortalApp(
      createClient: (apiKey) => ApiClient(
        client: http.Client(),
        // Proxy de solo lectura del propio portal (ADR-0035).
        base: Uri.base.resolve('/api/'),
        apiKey: apiKey,
      ),
      console: ConsoleApi(
        client: http.Client(),
        base: Uri.base.resolve('/api/'),
      ),
      status: StatusClient(
        client: http.Client(),
        // Mismo origen: el servidor del portal hace de proxy del estado (ADR-0034 §3).
        endpoint: Uri.base.resolve('/status/ready'),
      ),
    ),
  );
}

/// Aplicación del portal público de GeoStream.
class PortalApp extends StatelessWidget {
  /// Crea la aplicación.
  const PortalApp({
    super.key,
    required this.status,
    this.createClient,
    this.console,
  });

  /// Origen del estado en vivo.
  final StatusClient status;

  /// Cliente de la consola de desarrolladores (registro y API keys).
  final ConsoleApi? console;

  /// Cliente de la API para el acceso con API key.
  final ApiClientFactory? createClient;

  Route<void> _route(RouteSettings settings) {
    final Widget page = settings.name == '/console' && console != null
        ? DeveloperConsole(api: console!)
        : LandingPage(
            status: status,
            createClient: createClient,
            console: console,
          );
    return MaterialPageRoute<void>(settings: settings, builder: (_) => page);
  }

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'GeoStream',
      debugShowCheckedModeBanner: false,
      theme: ThemeData(
        useMaterial3: true,
        colorSchemeSeed: const Color(0xFF0B6E4F),
      ),
      darkTheme: ThemeData(
        useMaterial3: true,
        brightness: Brightness.dark,
        colorSchemeSeed: const Color(0xFF0B6E4F),
      ),
      // La consola tiene su propia dirección (`/#/console`): recargar la página o volver a ella retoma la
      // sesión en lugar de caer en la página de inicio. Solo se construye la ruta pedida: apilar la portada
      // debajo (fuera de pantalla y sin layout) hace fallar a `SelectionArea` al medirla.
      onGenerateInitialRoutes: (name) => [_route(RouteSettings(name: name))],
      onGenerateRoute: _route,
    );
  }
}
