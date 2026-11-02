import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:geostream_portal/src/console/console_api.dart';
import 'package:geostream_portal/src/platform/session_store.dart';
import 'package:geostream_portal/src/ui/developer_console.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';

const String _org = 'ten_0123456789abcdef0123456789abcdef';
const String _project = 'app_0123456789abcdef0123456789abcdef';
const String _token = 'gcs_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA';
const String _secret =
    'gsk_0123456789abcdef0123456789abcdef.AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA';
const String _password = 'a sturdy test password';

class _Call {
  _Call(this.method, this.path, this.auth, this.body);
  final String method;
  final String path;
  final String? auth;
  final Map<String, Object?> body;
}

/// Servidor simulado de la consola: cuentas con contraseña, organizaciones, proyectos y claves.
class _FakeConsole {
  final List<_Call> calls = [];
  bool disabled = false;
  bool tokenValid = true;
  bool hasPassword = true;
  bool projectActive = true;
  List<String> origins = [];
  String currentPassword = _password;
  final List<Map<String, Object?>> keys = [];

  _Call? last(String method, String suffix) => calls.cast<_Call?>().lastWhere(
    (c) => c!.method == method && c.path.endsWith(suffix),
    orElse: () => null,
  );

  Map<String, Object?> get _projectJson => {
    'id': _project,
    'organization_id': _org,
    'name': 'Flota',
    'status': projectActive ? 'ACTIVE' : 'DISABLED',
    'allowed_device_scopes': <String>[],
    'allowed_origins': origins,
  };

  Map<String, Object?> _keyJson(
    String label,
    List<Object?> scopes, {
    int? days,
  }) => {
    'id': 'key_${keys.length.toRadixString(16).padLeft(32, '0')}',
    'label': label,
    'prefix': 'gsk_01234567',
    'scopes': scopes,
    'created_at': '2026-11-02T10:00:00Z',
    'expires_at': days == null ? null : '2027-01-01T00:00:00Z',
    'revoked_at': null,
  };

  http.Response _json(int status, Object? body) => http.Response(
    jsonEncode(body),
    status,
    headers: {'content-type': 'application/json'},
  );

  Map<String, Object?> get _session => {
    'token': _token,
    'expires_at': '2026-11-03T00:00:00Z',
    'created': false,
    'account': {
      'id': 'acc_0123456789abcdef0123456789abcdef',
      'email': 'dev@example.test',
      'status': 'ACTIVE',
      'has_password': true,
      'created_at': '2026-11-01T10:00:00Z',
    },
  };

  Future<http.Response> handle(http.Request request) async {
    final body = request.body.isEmpty
        ? <String, Object?>{}
        : jsonDecode(request.body) as Map<String, Object?>;
    final path = request.url.path;
    calls.add(
      _Call(request.method, path, request.headers['Authorization'], body),
    );
    if (disabled) return http.Response('', 404);
    final route = '${request.method} $path';
    const base = '/api/v1/console';
    const org = '$base/organizations/$_org';
    switch (route) {
      case 'POST $base/auth/code':
        return _json(202, {'accepted': true});
      case 'POST $base/auth/verify':
        if ((body['password'] as String).length < 10) {
          return _json(422, {
            'code': 'VALIDATION_FAILED',
            'errors': [
              {'field': 'password', 'reason': 'out_of_range'},
            ],
          });
        }
        currentPassword = body['password']! as String;
        hasPassword = true;
        return _json(200, {..._session, 'created': true});
      case 'POST $base/auth/login':
        return _json(202, {'accepted': true});
      case 'POST $base/auth/login/verify':
        return body['code'] == '123456'
            ? _json(200, _session)
            : _json(401, {'code': 'UNAUTHENTICATED'});
      case 'GET $base/me':
        return tokenValid
            ? _json(200, {
                'id': 'acc_0123456789abcdef0123456789abcdef',
                'email': 'dev@example.test',
                'status': 'ACTIVE',
                'has_password': hasPassword,
                'created_at': '2026-11-01T10:00:00Z',
              })
            : _json(401, {'code': 'UNAUTHENTICATED'});
      case 'PUT $base/me/password':
        if (body['current_password'] != currentPassword && hasPassword) {
          return _json(403, {'code': 'PERMISSION_DENIED'});
        }
        currentPassword = body['new_password']! as String;
        hasPassword = true;
        return http.Response('', 204);
      case 'POST $base/auth/logout':
        return http.Response('', 204);
      case 'GET $base/organizations':
        return _json(200, {
          'data': [
            {
              'id': _org,
              'name': 'Acme',
              'role': 'OWNER',
              'created_at': '2026-11-01T10:00:00Z',
            },
          ],
        });
      case 'GET $org/projects':
        return _json(200, {
          'data': [_projectJson],
        });
      case 'GET $org/projects/$_project/keys':
        return _json(200, {'data': keys});
      case 'POST $org/projects/$_project/keys':
        final key = _keyJson(
          body['label']! as String,
          body['scopes']! as List<Object?>,
          days: body['expires_in_days'] as int?,
        );
        keys.add(key);
        return _json(201, {'key': key, 'secret': _secret});
      case 'PATCH $org/projects/$_project':
        if (body.containsKey('status')) {
          projectActive = body['status'] == 'ACTIVE';
        }
        if (body.containsKey('allowed_origins')) {
          origins = [
            for (final o in body['allowed_origins']! as List<Object?>)
              o! as String,
          ];
        }
        return _json(200, _projectJson);
    }
    if (request.method == 'DELETE' && path.contains('/keys/')) {
      keys.last['revoked_at'] = '2026-11-02T11:00:00Z';
      return http.Response('', 204);
    }
    return _json(404, {'code': 'NOT_FOUND'});
  }

  ConsoleApi api() => ConsoleApi(
    base: Uri.parse('https://portal.example/api/'),
    client: MockClient(handle),
  );
}

Future<void> _pump(
  WidgetTester tester,
  _FakeConsole fake, {
  SessionStore? store,
}) async {
  tester.view.physicalSize = const Size(1000, 1800);
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(
    MaterialApp(
      home: DeveloperConsole(
        api: fake.api(),
        store: store ?? MemorySessionStore(),
      ),
    ),
  );
  await tester.pumpAndSettle();
}

Finder _field(String label) => find.widgetWithText(TextField, label);

void main() {
  // covers: FR-CON-001, FR-CON-013
  testWidgets(
    'entrar es correo y contraseña, y después el código; la sesión se recuerda',
    (tester) async {
      final fake = _FakeConsole();
      final store = MemorySessionStore();
      await _pump(tester, fake, store: store);
      expect(find.text('Consola de GeoStream'), findsWidgets);

      await tester.enterText(_field('Correo'), 'dev@example.test');
      await tester.enterText(_field('Contraseña'), _password);
      await tester.tap(find.text('Continuar'));
      await tester.pumpAndSettle();
      // Entre los dos pasos: la contraseña ya no está en el formulario y el servidor la recibió una vez.
      expect(fake.last('POST', '/auth/login')!.body['password'], _password);
      expect(find.textContaining('te enviamos un código'), findsOneWidget);
      // La contraseña ya cumplió su papel: al volver al primer paso el campo está vacío.
      await tester.tap(find.text('Volver'));
      await tester.pumpAndSettle();
      expect(
        tester.widget<TextField>(_field('Contraseña')).controller!.text,
        isEmpty,
        reason: 'la contraseña no se conserva en el formulario',
      );
      await tester.enterText(_field('Contraseña'), _password);
      await tester.tap(find.text('Continuar'));
      await tester.pumpAndSettle();

      await tester.enterText(_field('Código'), '123456');
      await tester.tap(find.text('Entrar').last);
      await tester.pumpAndSettle();
      expect(find.text('Proyectos'), findsWidgets);
      expect(find.text('Acme'), findsOneWidget);
      expect(store.read(), _token, reason: 'la sesión sobrevive a la recarga');
    },
  );

  testWidgets('crear cuenta pide código, contraseña repetida y condiciones', (
    tester,
  ) async {
    final fake = _FakeConsole();
    final store = MemorySessionStore();
    await _pump(tester, fake, store: store);
    await tester.tap(find.text('Crear cuenta').first);
    await tester.pumpAndSettle();
    await tester.enterText(_field('Correo'), 'dev@example.test');
    await tester.tap(find.text('Enviarme un código'));
    await tester.pumpAndSettle();

    await tester.enterText(_field('Código'), '654321');
    await tester.enterText(_field('Elige una contraseña'), _password);
    await tester.enterText(
      _field('Repite la contraseña'),
      'otra distinta cualquiera',
    );
    await tester.tap(find.byType(CheckboxListTile));
    await tester.tap(find.text('Crear cuenta').last);
    await tester.pumpAndSettle();
    expect(find.textContaining('no coinciden'), findsOneWidget);
    expect(
      fake.last('POST', '/auth/verify'),
      isNull,
      reason: 'no se envía con contraseñas distintas',
    );

    await tester.enterText(_field('Repite la contraseña'), _password);
    await tester.tap(find.text('Crear cuenta').last);
    await tester.pumpAndSettle();
    final verify = fake.last('POST', '/auth/verify')!;
    expect(verify.body, {
      'email': 'dev@example.test',
      'code': '654321',
      'password': _password,
      'accept_terms': true,
    });
    expect(store.read(), _token);
    expect(find.text('Acme'), findsOneWidget);
  });

  testWidgets('una contraseña corta se rechaza con un mensaje claro', (
    tester,
  ) async {
    final fake = _FakeConsole();
    await _pump(tester, fake);
    await tester.tap(find.text('Crear cuenta').first);
    await tester.pumpAndSettle();
    await tester.enterText(_field('Correo'), 'dev@example.test');
    await tester.tap(find.text('Enviarme un código'));
    await tester.pumpAndSettle();
    await tester.enterText(_field('Código'), '654321');
    await tester.enterText(_field('Elige una contraseña'), 'corta');
    await tester.enterText(_field('Repite la contraseña'), 'corta');
    await tester.tap(find.byType(CheckboxListTile));
    await tester.tap(find.text('Crear cuenta').last);
    await tester.pumpAndSettle();
    expect(find.textContaining('entre 10 y 128 caracteres'), findsOneWidget);
  });

  testWidgets('al recargar se retoma la sesión guardada sin pedir nada', (
    tester,
  ) async {
    final fake = _FakeConsole();
    await _pump(tester, fake, store: MemorySessionStore(_token));
    expect(find.text('Acme'), findsOneWidget);
    expect(find.widgetWithText(TextField, 'Contraseña'), findsNothing);
    expect(fake.last('GET', '/me')!.auth, 'Bearer $_token');
  });

  testWidgets(
    'una sesión guardada que ya no vale se descarta y se pide entrar',
    (tester) async {
      final fake = _FakeConsole()..tokenValid = false;
      final store = MemorySessionStore(_token);
      await _pump(tester, fake, store: store);
      expect(store.read(), isNull);
      expect(find.textContaining('Tu sesión caducó'), findsOneWidget);
      expect(_field('Contraseña'), findsOneWidget);
    },
  );

  testWidgets('con la consola apagada el mensaje lo dice', (tester) async {
    final fake = _FakeConsole()..disabled = true;
    await _pump(tester, fake);
    await tester.enterText(_field('Correo'), 'dev@example.test');
    await tester.enterText(_field('Contraseña'), _password);
    await tester.tap(find.text('Continuar'));
    await tester.pumpAndSettle();
    expect(find.textContaining('todavía no está activa'), findsOneWidget);
  });

  testWidgets(
    'un proyecto admite varias API keys con permisos y caducidad distintos',
    (tester) async {
      final fake = _FakeConsole();
      await _pump(tester, fake, store: MemorySessionStore(_token));
      await tester.tap(find.text('Acme'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('Flota'));
      await tester.pumpAndSettle();
      expect(find.text('Este proyecto aún no tiene API keys.'), findsOneWidget);

      Future<void> create(String label, String preset, String expiry) async {
        await tester.tap(find.text('Nueva API key'));
        await tester.pumpAndSettle();
        await tester.enterText(
          _field('Etiqueta (por ejemplo, backend de producción)'),
          label,
        );
        await tester.tap(find.text(preset));
        await tester.pumpAndSettle();
        if (expiry.isNotEmpty) {
          await tester.tap(find.text('No caduca'));
          await tester.pumpAndSettle();
          await tester.tap(find.text(expiry).last);
          await tester.pumpAndSettle();
        }
        await tester.tap(find.text('Generar'));
        await tester.pumpAndSettle();
        // El secreto se muestra una vez, en un diálogo.
        expect(find.text(_secret), findsOneWidget);
        await tester.tap(find.text('Ya la guardé'));
        await tester.pumpAndSettle();
        expect(
          find.text(_secret),
          findsNothing,
          reason: 'no se vuelve a mostrar',
        );
      }

      await create('backend', 'Entregas completas', '');
      expect(jsonDecode(jsonEncode(fake.last('POST', '/keys')!.body)), {
        'label': 'backend',
        'scopes': [
          'sessions:read',
          'sessions:write',
          'locations:read',
          'tokens:mint',
        ]..sort(),
      });
      await create('panel', 'Solo lectura', 'A los 30 días');
      final second = fake.last('POST', '/keys')!.body;
      expect(second['expires_in_days'], 30);
      expect(second['scopes'], ['locations:read', 'sessions:read']);

      expect(find.text('backend'), findsOneWidget);
      expect(find.text('panel'), findsOneWidget);
      expect(fake.keys.length, 2);
    },
  );

  testWidgets('revocar pide confirmación y solo entonces llama al servidor', (
    tester,
  ) async {
    final fake = _FakeConsole();
    fake.keys.add(fake._keyJson('backend', ['sessions:read']));
    await _pump(tester, fake, store: MemorySessionStore(_token));
    await tester.tap(find.text('Acme'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Flota'));
    await tester.pumpAndSettle();
    await tester.tap(find.byTooltip('Revocar'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Cancelar'));
    await tester.pumpAndSettle();
    expect(fake.last('DELETE', fake.keys.first['id']! as String), isNull);

    await tester.tap(find.byTooltip('Revocar'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Revocar').last);
    await tester.pumpAndSettle();
    expect(fake.calls.where((c) => c.method == 'DELETE').length, 1);
    expect(find.text('Revocadas'), findsOneWidget);
  });

  testWidgets(
    'la configuración del proyecto guarda orígenes y deshabilita con confirmación',
    (tester) async {
      final fake = _FakeConsole();
      await _pump(tester, fake, store: MemorySessionStore(_token));
      await tester.tap(find.text('Acme'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('Flota'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('Configuración'));
      await tester.pumpAndSettle();

      expect(find.text('https://geostream.gravitalcloud.com'), findsOneWidget);
      final originsField = find.byType(TextField).at(1);
      await tester.enterText(
        originsField,
        'https://app.example.test\nhttps://admin.example.test',
      );
      await tester.tap(find.text('Guardar orígenes'));
      await tester.pumpAndSettle();
      expect(
        fake.last('PATCH', '/projects/$_project')!.body['allowed_origins'],
        ['https://app.example.test', 'https://admin.example.test'],
      );

      await tester.tap(find.byType(Switch));
      await tester.pumpAndSettle();
      expect(find.text('¿Deshabilitar el proyecto?'), findsOneWidget);
      await tester.tap(find.text('Deshabilitar'));
      await tester.pumpAndSettle();
      expect(fake.projectActive, isFalse);
    },
  );

  testWidgets('la cuenta cambia la contraseña con la actual y cierra sesión', (
    tester,
  ) async {
    final fake = _FakeConsole();
    final store = MemorySessionStore(_token);
    await _pump(tester, fake, store: store);
    await tester.tap(find.text('Cuenta').first);
    await tester.pumpAndSettle();
    expect(find.text('dev@example.test'), findsWidgets);

    await tester.enterText(
      _field('Contraseña actual'),
      'no es esta contraseña',
    );
    await tester.enterText(
      _field('Contraseña nueva (mínimo 10 caracteres)'),
      'la contraseña nueva',
    );
    await tester.enterText(
      _field('Repite la contraseña nueva'),
      'la contraseña nueva',
    );
    await tester.tap(find.text('Guardar contraseña'));
    await tester.pumpAndSettle();
    expect(
      find.textContaining('contraseña actual no es correcta'),
      findsOneWidget,
    );

    await tester.enterText(_field('Contraseña actual'), _password);
    await tester.tap(find.text('Guardar contraseña'));
    await tester.pumpAndSettle();
    expect(find.textContaining('Contraseña cambiada'), findsOneWidget);
    expect(fake.last('PUT', '/me/password')!.body, {
      'current_password': _password,
      'new_password': 'la contraseña nueva',
    });

    await tester.tap(find.text('Cerrar sesión').first);
    await tester.pumpAndSettle();
    expect(store.read(), isNull);
    expect(_field('Contraseña'), findsOneWidget);
  });

  test('el cliente no manda nada autenticado sin sesión', () async {
    final api = _FakeConsole().api();
    expect(
      () => api.organizations(),
      throwsA(
        isA<ConsoleApiException>().having((e) => e.status, 'status', 401),
      ),
    );
  });
}
