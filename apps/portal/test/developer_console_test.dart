import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:geostream_portal/src/console/console_api.dart';
import 'package:geostream_portal/src/ui/developer_console.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';

const String _org =
    '{"id":"ten_0123456789abcdef0123456789abcdef","name":"Acme","role":"OWNER","created_at":"2026-11-01T10:00:00Z"}';
const String _project =
    '{"id":"app_0123456789abcdef0123456789abcdef","name":"Flota","status":"ACTIVE"}';
const String _secret =
    'gsk_0123456789abcdef0123456789abcdef.AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA';

class _Call {
  _Call(this.method, this.path, this.auth, this.body);
  final String method;
  final String path;
  final String? auth;
  final String body;
}

/// Servidor simulado del contrato: devuelve lo que el contrato dice para cada ruta.
ConsoleApi _api(
  List<_Call> calls, {
  int verifyStatus = 200,
  bool disabled = false,
}) {
  return ConsoleApi(
    base: Uri.parse('https://portal.example/api/'),
    client: MockClient((request) async {
      calls.add(
        _Call(
          request.method,
          request.url.path,
          request.headers['Authorization'],
          request.body,
        ),
      );
      if (disabled) return http.Response('', 404);
      final route = '${request.method} ${request.url.path}';
      http.Response json(int status, String body) => http.Response(
        body,
        status,
        headers: {'content-type': 'application/json'},
      );
      switch (route) {
        case 'POST /api/v1/console/auth/code':
          return json(202, '{"accepted":true}');
        case 'POST /api/v1/console/auth/verify':
          if (verifyStatus == 401) {
            return json(401, '{"code":"UNAUTHENTICATED"}');
          }
          if (verifyStatus == 422) {
            return json(
              422,
              '{"code":"VALIDATION_FAILED","errors":[{"field":"accept_terms","reason":"required"}]}',
            );
          }
          return json(
            200,
            '{"token":"gcs_${'A' * 43}","expires_at":"2026-11-02T00:00:00Z","created":true,'
            '"account":{"id":"acc_0123456789abcdef0123456789abcdef","email":"dev@example.test","status":"ACTIVE","created_at":"2026-11-01T10:00:00Z"}}',
          );
        case 'GET /api/v1/console/organizations':
          return json(200, '{"data":[$_org]}');
        case 'GET /api/v1/console/organizations/ten_0123456789abcdef0123456789abcdef/projects':
          return json(200, '{"data":[$_project]}');
        case 'GET /api/v1/console/organizations/ten_0123456789abcdef0123456789abcdef/projects/app_0123456789abcdef0123456789abcdef/keys':
          return json(200, '{"data":[]}');
        case 'POST /api/v1/console/organizations/ten_0123456789abcdef0123456789abcdef/projects/app_0123456789abcdef0123456789abcdef/keys':
          return json(
            201,
            '{"key":{"id":"key_0123456789abcdef0123456789abcdef","label":"backend","prefix":"gsk_01234567","scopes":["sessions:read"],"created_at":"2026-11-01T10:00:00Z","expires_at":null,"revoked_at":null},"secret":"$_secret"}',
          );
      }
      return json(404, '{"code":"NOT_FOUND"}');
    }),
  );
}

Future<void> _pump(WidgetTester tester, ConsoleApi api) async {
  tester.view.physicalSize = const Size(900, 1600);
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(MaterialApp(home: DeveloperConsole(api: api)));
}

void main() {
  // covers: FR-CON-001, FR-CON-004, FR-CON-012
  testWidgets('registro, organización, proyecto y API key de punta a punta', (
    tester,
  ) async {
    final calls = <_Call>[];
    await _pump(tester, _api(calls));

    await tester.enterText(find.byType(TextField), 'dev@example.test');
    await tester.tap(find.text('Enviarme un código'));
    await tester.pumpAndSettle();
    expect(find.text('Código'), findsOneWidget);

    await tester.enterText(find.widgetWithText(TextField, 'Código'), '123456');
    await tester.tap(find.byType(CheckboxListTile));
    await tester.tap(find.text('Entrar'));
    await tester.pumpAndSettle();
    final verify = calls.firstWhere((c) => c.path.endsWith('/auth/verify'));
    expect(jsonDecode(verify.body), {
      'email': 'dev@example.test',
      'code': '123456',
      'accept_terms': true,
    });
    expect(find.text('Acme'), findsOneWidget);
    expect(find.text('dev@example.test'), findsOneWidget);

    await tester.tap(find.text('Acme'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Flota'));
    await tester.pumpAndSettle();

    await tester.enterText(
      find.widgetWithText(
        TextField,
        'Etiqueta (por ejemplo, backend de producción)',
      ),
      'backend',
    );
    await tester.ensureVisible(find.text('sessions:read'));
    await tester.tap(find.text('sessions:read'));
    await tester.ensureVisible(find.text('Generar API key'));
    await tester.tap(find.text('Generar API key'));
    await tester.pumpAndSettle();

    // El secreto se muestra una vez, en un diálogo.
    expect(find.text(_secret), findsOneWidget);
    final create = calls.lastWhere(
      (c) => c.method == 'POST' && c.path.endsWith('/keys'),
    );
    expect(jsonDecode(create.body), {
      'label': 'backend',
      'scopes': ['sessions:read'],
    });
    expect(create.auth, 'Bearer gcs_${'A' * 43}');

    await tester.tap(find.text('Ya la guardé'));
    await tester.pumpAndSettle();
    expect(find.text(_secret), findsNothing);
  });

  testWidgets('un código incorrecto muestra un mensaje y no abre sesión', (
    tester,
  ) async {
    await _pump(tester, _api([], verifyStatus: 401));
    await tester.enterText(find.byType(TextField), 'dev@example.test');
    await tester.tap(find.text('Enviarme un código'));
    await tester.pumpAndSettle();
    await tester.enterText(find.widgetWithText(TextField, 'Código'), '000000');
    await tester.tap(find.text('Entrar'));
    await tester.pumpAndSettle();
    expect(find.textContaining('no es válido o ya caducó'), findsOneWidget);
    expect(find.text('Organizaciones'), findsNothing);
  });

  testWidgets('si falta aceptar las condiciones lo pide con claridad', (
    tester,
  ) async {
    await _pump(tester, _api([], verifyStatus: 422));
    await tester.enterText(find.byType(TextField), 'dev@example.test');
    await tester.tap(find.text('Enviarme un código'));
    await tester.pumpAndSettle();
    await tester.enterText(find.widgetWithText(TextField, 'Código'), '123456');
    await tester.tap(find.text('Entrar'));
    await tester.pumpAndSettle();
    expect(find.textContaining('aceptar las condiciones'), findsWidgets);
  });

  testWidgets(
    'si la consola no está activada lo dice en vez de un error genérico',
    (tester) async {
      await _pump(tester, _api([], disabled: true));
      await tester.enterText(find.byType(TextField), 'dev@example.test');
      await tester.tap(find.text('Enviarme un código'));
      await tester.pumpAndSettle();
      expect(find.textContaining('todavía no está activo'), findsOneWidget);
      expect(find.text('Código'), findsNothing);
    },
  );

  test('el cliente nunca manda la sesión sin haberla abierto', () async {
    final api = _api([]);
    expect(
      () => api.organizations(),
      throwsA(
        isA<ConsoleApiException>().having((e) => e.status, 'status', 401),
      ),
    );
  });
}
