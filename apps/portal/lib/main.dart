import 'package:flutter/material.dart';
import 'package:http/http.dart' as http;
import 'package:maplibre_gl/maplibre_gl.dart';

import 'src/api/api_client.dart';
import 'src/status/status_client.dart';
import 'src/ui/console_page.dart';
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
  const PortalApp({super.key, required this.status, this.createClient});

  /// Origen del estado en vivo.
  final StatusClient status;

  /// Cliente de la API para el acceso con API key.
  final ApiClientFactory? createClient;

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
      home: LandingPage(status: status, createClient: createClient),
    );
  }
}
