import 'package:flutter/material.dart';

import '../location/tracking_controller.dart';
import 'connection_page.dart';
import 'geofences_page.dart';
import 'live_map_page.dart';

/// Navegación inferior: Conexión, Mapa y Geocercas.
class HomePage extends StatefulWidget {
  /// Crea la página.
  const HomePage({super.key, required this.controller});

  /// Estado compartido.
  final TrackingController controller;

  @override
  State<HomePage> createState() => _HomePageState();
}

class _HomePageState extends State<HomePage> {
  int _tab = 0;

  @override
  Widget build(BuildContext context) {
    final pages = [
      ConnectionPage(controller: widget.controller),
      LiveMapPage(controller: widget.controller),
      GeofencesPage(controller: widget.controller),
    ];
    return Scaffold(
      body: SafeArea(
        child: IndexedStack(index: _tab, children: pages),
      ),
      bottomNavigationBar: NavigationBar(
        selectedIndex: _tab,
        onDestinationSelected: (i) => setState(() => _tab = i),
        destinations: const [
          NavigationDestination(icon: Icon(Icons.link), label: 'Conexión'),
          NavigationDestination(icon: Icon(Icons.map), label: 'Mapa'),
          NavigationDestination(icon: Icon(Icons.fence), label: 'Geocercas'),
        ],
      ),
    );
  }
}
