import 'package:flutter_test/flutter_test.dart';
import 'package:geostream_portal/src/status/readiness.dart';
import 'package:geostream_portal/src/status/status_client.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';

final DateTime _now = DateTime.utc(2026, 11, 1, 12);

StatusClient _client(MockClient mock, {Duration? timeout}) => StatusClient(
  client: mock,
  endpoint: Uri.parse('https://portal.example/status/ready'),
  clock: () => _now,
  timeout: timeout ?? const Duration(seconds: 5),
);

void main() {
  // covers: FR-PRT-002
  test('un 200 con ReadinessStatus válido es operativo', () async {
    final snapshot = await _client(
      MockClient(
        (_) async => http.Response(
          '{"status":"ready","checks":{"postgresql":"ok","bus":"not_configured"}}',
          200,
        ),
      ),
    ).fetch();
    expect(snapshot, isA<ReadinessKnown>());
    final known = snapshot as ReadinessKnown;
    expect(known.ready, isTrue);
    expect(known.checks['bus'], CheckState.notConfigured);
    expect(known.checkedAt, _now);
  });

  test('un 503 con not_ready es degradado y conserva el detalle', () async {
    final snapshot = await _client(
      MockClient(
        (_) async => http.Response(
          '{"status":"not_ready","checks":{"postgresql":"ok","ephemeral":"failing"}}',
          503,
        ),
      ),
    ).fetch();
    final known = snapshot as ReadinessKnown;
    expect(known.ready, isFalse);
    expect(known.checks['ephemeral'], CheckState.failing);
  });

  test('si la red falla, no hay datos y nunca disponible', () async {
    final snapshot = await _client(
      MockClient((_) async => throw http.ClientException('sin red')),
    ).fetch();
    expect(snapshot, isA<ReadinessUnknown>());
  });

  test('si el servicio tarda más que el tiempo máximo, no hay datos', () async {
    final snapshot = await _client(
      MockClient(
        (_) => Future<http.Response>.delayed(
          const Duration(seconds: 1),
          () => http.Response('{"status":"ready","checks":{}}', 200),
        ),
      ),
      timeout: const Duration(milliseconds: 20),
    ).fetch();
    expect(snapshot, isA<ReadinessUnknown>());
  });

  test('otros códigos HTTP no son disponibilidad', () async {
    for (final code in [404, 500, 502, 504]) {
      final snapshot = await _client(
        MockClient(
          (_) async => http.Response('{"status":"ready","checks":{}}', code),
        ),
      ).fetch();
      expect(snapshot, isA<ReadinessUnknown>(), reason: 'HTTP $code');
    }
  });

  test('un cuerpo que no cumple el esquema es sin datos', () async {
    const bodies = [
      '',
      'no es json',
      '[]',
      '{"ready":true,"dependencies":[]}',
      '{"status":"up","checks":{}}',
      '{"status":"ready"}',
      '{"status":"ready","checks":{"postgresql":"fine"}}',
      '{"status":"ready","checks":{"postgresql":true}}',
    ];
    for (final body in bodies) {
      final snapshot = await _client(
        MockClient((_) async => http.Response(body, 200)),
      ).fetch();
      expect(snapshot, isA<ReadinessUnknown>(), reason: body);
    }
  });
}
