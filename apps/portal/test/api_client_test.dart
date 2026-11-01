import 'package:flutter_test/flutter_test.dart';
import 'package:geostream_portal/src/api/api_client.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';

ApiClient _client(MockClient mock) => ApiClient(
  client: mock,
  base: Uri.parse('https://portal.example/api/'),
  apiKey: 'gsk_test_key',
);

const String _session =
    '{"id":"ses_0123456789abcdef0123456789abcdef","mode":"MANY_TO_MANY","status":"ACTIVE",'
    '"active_member_count":2,"expires_at":"2026-11-02T10:00:00.000Z"}';

String _item(String member, num lat, num lon) =>
    '{"member_id":"$member","external_subject_id":"rider-1","age_ms":2500,'
    '"sample":{"sequence":1,"captured_at":"2026-11-01T10:00:00.000Z",'
    '"position":{"latitude":$lat,"longitude":$lon,"horizontal_accuracy_m":8.5},"quality":{}}}';

void main() {
  // covers: FR-PRT-004
  test('lista sesiones con la key en Authorization y solo con GET', () async {
    late http.Request seen;
    final sessions = await _client(
      MockClient((request) async {
        seen = request;
        return http.Response('{"data":[$_session],"next_cursor":null}', 200);
      }),
    ).listSessions();
    expect(seen.method, 'GET');
    expect(seen.headers['Authorization'], 'Bearer gsk_test_key');
    expect(seen.url.path, '/api/v1/sessions');
    expect(seen.url.queryParameters['status'], 'ACTIVE');
    expect(sessions.single.activeMemberCount, 2);
    expect(sessions.single.expiresAt, DateTime.utc(2026, 11, 2, 10));
  });

  test('la key no viaja en la URL', () async {
    late Uri url;
    await _client(
      MockClient((request) async {
        url = request.url;
        return http.Response('{"data":[],"next_cursor":null}', 200);
      }),
    ).listSessions();
    expect(url.toString(), isNot(contains('gsk_test_key')));
  });

  test('401 y 403 son key rechazada', () async {
    for (final code in [401, 403]) {
      expect(
        _client(MockClient((_) async => http.Response('{}', code)))
            .listSessions(),
        throwsA(isA<ApiKeyRejected>()),
        reason: 'HTTP $code',
      );
    }
  });

  test('red caída, 5xx o cuerpo inválido son servicio no disponible', () async {
    final failures = <MockClient>[
      MockClient((_) async => throw http.ClientException('sin red')),
      MockClient((_) async => http.Response('', 503)),
      MockClient((_) async => http.Response('no es json', 200)),
      MockClient((_) async => http.Response('{"data":"x"}', 200)),
    ];
    for (final mock in failures) {
      expect(_client(mock).listSessions(), throwsA(isA<ApiUnavailable>()));
    }
  });

  test(
    'el snapshot descarta elementos inválidos y coordenadas fuera de rango',
    () async {
      final positions = await _client(
        MockClient((request) async {
          expect(request.url.path, '/api/v1/sessions/ses_1/snapshot');
          return http.Response(
            '{"session_id":"ses_1","generated_at":"2026-11-01T10:00:00.000Z","items":['
            '${_item("mem_a", 8.98, -79.52)},'
            '${_item("mem_b", 91, 0)},'
            '${_item("mem_c", 0, 181)},'
            '{"member_id":"mem_d"}]}',
            200,
          );
        }),
      ).snapshot('ses_1');
      expect(positions.map((p) => p.memberId), ['mem_a']);
      expect(positions.single.accuracyMeters, 8.5);
      expect(positions.single.ageMs, 2500);
    },
  );
}
