import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:geostream_portal/src/status/status_client.dart';
import 'package:geostream_portal/src/ui/landing_page.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';

StatusClient _status(http.Response Function() respond) => StatusClient(
  client: MockClient((_) async => respond()),
  endpoint: Uri.parse('https://portal.example/status/ready'),
  clock: () => DateTime.utc(2026, 11, 1, 12),
);

Future<void> _pump(WidgetTester tester, StatusClient status) async {
  tester.view.physicalSize = const Size(1200, 4000);
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(MaterialApp(home: LandingPage(status: status)));
  await tester.pumpAndSettle();
}

void main() {
  // covers: FR-PRT-001, FR-PRT-002
  testWidgets('muestra operativo solo con un ReadinessStatus listo', (
    tester,
  ) async {
    await _pump(
      tester,
      _status(
        () => http.Response(
          '{"status":"ready","checks":{"postgresql":"ok","bus":"not_configured"}}',
          200,
        ),
      ),
    );
    expect(find.text('Operativo'), findsWidgets);
    expect(find.text('Sin datos'), findsNothing);
    expect(
      find.textContaining('no un porcentaje de disponibilidad'),
      findsOneWidget,
    );
  });

  testWidgets('con la API caída no dice operativo', (tester) async {
    await _pump(tester, _status(() => http.Response('', 502)));
    expect(find.text('Sin datos'), findsOneWidget);
    expect(find.text('Operativo'), findsNothing);
    expect(find.textContaining('no significa que esté caído'), findsOneWidget);
  });

  testWidgets('con un 503 se declara degradado', (tester) async {
    await _pump(
      tester,
      _status(
        () => http.Response(
          '{"status":"not_ready","checks":{"postgresql":"failing"}}',
          503,
        ),
      ),
    );
    expect(find.text('Degradado'), findsOneWidget);
    expect(find.text('Con fallos'), findsOneWidget);
  });

  testWidgets('publica los contratos y no promete lo que aún no existe', (
    tester,
  ) async {
    await _pump(tester, _status(() => http.Response('', 502)));
    expect(find.text('Referencia de la API REST'), findsOneWidget);
    expect(find.text('Contratos Protobuf'), findsOneWidget);
    // Lo que no está implementado se declara «En camino», no «Hoy».
    expect(find.text('En camino'), findsNWidgets(2));
    expect(find.text('Hoy'), findsNWidgets(6));
  });
}
