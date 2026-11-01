import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:geostream_portal/src/api/api_client.dart';
import 'package:geostream_portal/src/map/style_loader.dart';
import 'package:geostream_portal/src/ui/console_page.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';

const String _session =
    '{"id":"ses_0123456789abcdef0123456789abcdef","mode":"MANY_TO_MANY","status":"ACTIVE",'
    '"active_member_count":3,"expires_at":"2026-11-02T10:00:00.000Z"}';

ApiClientFactory _factory(int status, String body, {List<String>? keys}) =>
    (apiKey) {
      keys?.add(apiKey);
      return ApiClient(
        client: MockClient((_) async => http.Response(body, status)),
        base: Uri.parse('https://portal.example/api/'),
        apiKey: apiKey,
      );
    };

Future<void> _pump(WidgetTester tester, ApiClientFactory factory) async {
  await tester.pumpWidget(
    MaterialApp(
      home: ConsolePage(
        createClient: factory,
        mapBuilder: (client, session) =>
            Scaffold(body: Text('mapa ${session.id}')),
      ),
    ),
  );
}

void main() {
  // covers: FR-PRT-004
  testWidgets('con una key válida muestra las sesiones y vacía el campo', (
    tester,
  ) async {
    final keys = <String>[];
    await _pump(
      tester,
      _factory(200, '{"data":[$_session],"next_cursor":null}', keys: keys),
    );
    await tester.enterText(find.byType(TextField), 'gsk_secret');
    await tester.tap(find.text('Entrar'));
    await tester.pumpAndSettle();
    expect(keys, ['gsk_secret']);
    expect(find.text('ses_0123456789abcdef0123456789abcdef'), findsOneWidget);
    expect(find.textContaining('3 miembros activos'), findsOneWidget);
    // La key no se muestra en ninguna parte de la pantalla.
    expect(find.textContaining('gsk_secret'), findsNothing);
  });

  testWidgets('el campo de la key está oculto y sin sugerencias', (
    tester,
  ) async {
    await _pump(tester, _factory(200, '{"data":[],"next_cursor":null}'));
    final field = tester.widget<TextField>(find.byType(TextField));
    expect(field.obscureText, isTrue);
    expect(field.enableSuggestions, isFalse);
    expect(field.autocorrect, isFalse);
  });

  testWidgets('una key rechazada muestra el motivo y no entra', (tester) async {
    await _pump(tester, _factory(401, '{}'));
    await tester.enterText(find.byType(TextField), 'gsk_bad');
    await tester.tap(find.text('Entrar'));
    await tester.pumpAndSettle();
    expect(find.textContaining('no fue aceptada'), findsOneWidget);
    expect(find.text('Mis sesiones'), findsOneWidget);
    expect(find.byType(ListTile), findsNothing);
  });

  testWidgets('Salir descarta las sesiones y vuelve al formulario', (
    tester,
  ) async {
    await _pump(
      tester,
      _factory(200, '{"data":[$_session],"next_cursor":null}'),
    );
    await tester.enterText(find.byType(TextField), 'gsk_secret');
    await tester.tap(find.text('Entrar'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Salir'));
    await tester.pumpAndSettle();
    expect(find.text('Entra con tu API key'), findsOneWidget);
    expect(find.byType(ListTile), findsNothing);
  });

  testWidgets('tocar una sesión abre su mapa', (tester) async {
    await _pump(
      tester,
      _factory(200, '{"data":[$_session],"next_cursor":null}'),
    );
    await tester.enterText(find.byType(TextField), 'gsk_secret');
    await tester.tap(find.text('Entrar'));
    await tester.pumpAndSettle();
    await tester.tap(find.byType(ListTile));
    await tester.pumpAndSettle();
    expect(
      find.text('mapa ses_0123456789abcdef0123456789abcdef'),
      findsOneWidget,
    );
  });

  // covers: FR-PRT-004 (la key nunca se guarda), 10 T-38
  test('el código del portal no usa ningún almacenamiento del navegador', () {
    const forbidden = [
      'localStorage',
      'sessionStorage',
      'shared_preferences',
      'document.cookie',
      'indexedDB',
      'window.name',
    ];
    final offenders = <String>[];
    for (final file in Directory(
      'lib',
    ).listSync(recursive: true).whereType<File>()) {
      if (!file.path.endsWith('.dart')) continue;
      final text = file.readAsStringSync();
      for (final word in forbidden) {
        if (text.contains(word)) offenders.add('${file.path}: $word');
      }
    }
    expect(offenders, isEmpty);
  });

  // covers: FR-MAP-007
  test('el estilo se hace absoluto con el origen y no apunta a servidores ajenos', () {
    const style =
        '{"sources":{"g":{"tiles":["/tiles/panama/{z}/{x}/{y}"]}},"glyphs":"/tiles/font/{fontstack}/{range}"}';
    final absolute = absolutizeStyle(style, 'https://portal.example');
    expect(
      absolute,
      contains('"https://portal.example/tiles/panama/{z}/{x}/{y}"'),
    );
    expect(
      absolute,
      contains('"https://portal.example/tiles/font/{fontstack}/{range}"'),
    );
    expect(absolute, isNot(contains('openstreetmap.org/')));
  });
}
