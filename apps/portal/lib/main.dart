import 'package:flutter/material.dart';
import 'package:http/http.dart' as http;

import 'src/status/status_client.dart';
import 'src/ui/landing_page.dart';

void main() {
  runApp(PortalApp(
    status: StatusClient(
      client: http.Client(),
      // Mismo origen: el servidor del portal hace de proxy del estado (ADR-0034 §3).
      endpoint: Uri.base.resolve('/status/ready'),
    ),
  ));
}

/// Aplicación del portal público de GeoStream.
class PortalApp extends StatelessWidget {
  /// Crea la aplicación.
  const PortalApp({super.key, required this.status});

  /// Origen del estado en vivo.
  final StatusClient status;

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
      home: LandingPage(status: status),
    );
  }
}
