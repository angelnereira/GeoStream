import 'package:flutter/material.dart';

import 'src/api/geostream_api.dart';
import 'src/location/geolocator_source.dart';
import 'src/location/tracking_controller.dart';
import 'src/storage/app_store.dart';
import 'src/ui/home_page.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  final kv = await PrefsStore.load();
  final controller = TrackingController(
    store: AppStore(kv),
    sequences: await SequenceAllocator.open(kv),
    apiFactory: (base, token) =>
        GeoStreamApi(baseUrl: base, deviceToken: token),
    fixSource: geolocatorFixes,
  );
  runApp(GeoStreamTesterApp(controller: controller));
}

/// Raíz de la app de pruebas.
class GeoStreamTesterApp extends StatelessWidget {
  /// Crea la app con su [controller].
  const GeoStreamTesterApp({super.key, required this.controller});

  /// Estado compartido.
  final TrackingController controller;

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'GeoStream Tester',
      theme: ThemeData(colorSchemeSeed: Colors.teal, useMaterial3: true),
      home: HomePage(controller: controller),
    );
  }
}
