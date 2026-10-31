import 'dart:async';
import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:geostream_tester/src/api/geostream_api.dart';
import 'package:geostream_tester/src/geo/geodesy.dart';
import 'package:geostream_tester/src/geo/geofence.dart';
import 'package:geostream_tester/src/location/sample_mapper.dart';
import 'package:geostream_tester/src/location/tracking_controller.dart';
import 'package:geostream_tester/src/storage/app_store.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';

const _token = 'test-device-token-secret';

/// Servidor falso que registra las peticiones y responde como el contrato.
class FakeServer {
  final List<http.Request> requests = [];
  int ingestStatus = 200;

  late final MockClient client = MockClient((req) async {
    requests.add(req);
    final path = req.url.path;
    if (path == '/health/ready') {
      return http.Response(jsonEncode({'status': 'ok'}), 200);
    }
    expect(req.headers['authorization'], 'Bearer $_token');
    if (req.method == 'POST' && path == '/v1/sessions') {
      return http.Response(jsonEncode({'id': 'ses_abc'}), 201);
    }
    if (req.method == 'PUT' &&
        path == '/v1/sessions/ses_abc/members/me/sharing') {
      return http.Response(jsonEncode({'sharing': true}), 200);
    }
    if (req.method == 'POST' && path == '/v1/ingest/locations') {
      if (ingestStatus != 200) {
        return http.Response(
          jsonEncode({
            'type': 'https://geostream.dev/problems/unauthenticated',
            'title': 'Credencial inválida',
            'status': ingestStatus,
            'code': 'UNAUTHENTICATED',
            'request_id': 'req_${'0' * 32}',
          }),
          ingestStatus,
        );
      }
      final body = jsonDecode(req.body) as Map<String, dynamic>;
      final samples = (body['samples'] as List).cast<Map<String, dynamic>>();
      return http.Response(
        jsonEncode({
          'session_id': body['session_id'],
          'results': [
            for (final s in samples)
              {
                'sequence': s['sequence'],
                'status': 'ACCEPTED',
                'error_code': null,
              },
          ],
        }),
        200,
      );
    }
    if (req.method == 'GET' && path == '/v1/sessions/ses_abc/snapshot') {
      return http.Response(
        jsonEncode({
          'session_id': 'ses_abc',
          'generated_at': '2026-10-31T12:00:00.000Z',
          'items': [
            {
              'member_id': 'mem_1',
              'external_subject_id': 'user-1',
              'received_at': '2026-10-31T12:00:00.000Z',
              'age_ms': 120,
              'sample': {
                'sequence': 1,
                'captured_at': '2026-10-31T12:00:00.000Z',
                'position': {
                  'latitude': 8.98,
                  'longitude': -79.52,
                  'horizontal_accuracy_m': 5,
                },
                'quality': {'kind': 'MEASURED'},
              },
            },
          ],
        }),
        200,
      );
    }
    return http.Response('', 404);
  });
}

RawFix _fix(GeoPoint p, int i) => RawFix(
  latitude: p.latitude,
  longitude: p.longitude,
  accuracyM: 5,
  timestamp: DateTime.utc(2026, 10, 31).add(Duration(seconds: i)),
);

void main() {
  group('GeoStreamApi', () {
    test(
      'error del servidor → excepción con code y request_id, sin token',
      () async {
        final server = FakeServer()..ingestStatus = 401;
        final api = GeoStreamApi(
          baseUrl: Uri.parse('https://geo.test'),
          deviceToken: _token,
          client: server.client,
        );
        final error = await api
            .ingest('ses_abc', [
              toLocationSample(_fix(const GeoPoint(1, 1), 0), 1)!,
            ])
            .then<Object?>((_) => null, onError: (Object e) => e);
        expect(error, isA<GeoStreamApiException>());
        final e = error! as GeoStreamApiException;
        expect(e.status, 401);
        expect(e.code, 'UNAUTHENTICATED');
        expect(e.toString(), isNot(contains(_token)));
        expect(e.toString(), isNot(contains('1.0')));
      },
    );

    test('createSession manda Idempotency-Key y snapshot se parsea', () async {
      final server = FakeServer();
      final api = GeoStreamApi(
        baseUrl: Uri.parse('https://geo.test/'),
        deviceToken: _token,
        client: server.client,
      );
      expect(await api.createSession(), 'ses_abc');
      expect(
        server.requests.last.headers['idempotency-key'],
        matches(RegExp(r'^[A-Za-z0-9_\-:.]{1,255}$')),
      );
      expect(jsonDecode(server.requests.last.body), {'mode': 'ONE_TO_MANY'});
      final snap = await api.snapshot('ses_abc');
      expect(snap.single.memberId, 'mem_1');
      expect(snap.single.accuracyM, 5);
    });
  });

  group('TrackingController', () {
    late FakeServer server;
    late StreamController<RawFix> fixes;
    late TrackingController c;

    setUp(() async {
      server = FakeServer();
      fixes = StreamController<RawFix>();
      final kv = MemoryStore();
      c = TrackingController(
        store: AppStore(kv),
        sequences: await SequenceAllocator.open(kv),
        apiFactory: (base, token) => GeoStreamApi(
          baseUrl: base,
          deviceToken: token,
          client: server.client,
        ),
        fixSource: () => fixes.stream,
        flushInterval: const Duration(hours: 1),
        snapshotInterval: const Duration(hours: 1),
      );
    });

    tearDown(() => c.dispose());

    test('compartir está apagado por defecto y requiere sesión', () async {
      expect(c.sharing, isFalse);
      expect(await c.connect('https://geo.test', _token), isTrue);
      await c.startSharing();
      expect(c.sharing, isFalse, reason: 'sin sesión no se comparte');
    });

    test(
      'flujo completo: sesión → compartir → ingesta → geocerca local',
      () async {
        await c.connect('https://geo.test', _token);
        await c.createSession();
        expect(c.sessionId, 'ses_abc');
        const center = GeoPoint(8.98, -79.52);
        await c.upsertFence(
          const LocalGeofence(
            id: 'f1',
            name: 'Casa',
            shape: CircleShape(center, 100),
          ),
        );
        await c.startSharing();
        expect(c.sharing, isTrue);
        final sharingReq = server.requests.lastWhere((r) => r.method == 'PUT');
        expect(jsonDecode(sharingReq.body), {'sharing': true});

        fixes
          ..add(_fix(center, 1))
          ..add(_fix(center, 2));
        await pumpEventQueue();
        await c.flush();
        expect(c.accepted, 2);
        final ingest = server.requests.lastWhere(
          (r) => r.url.path == '/v1/ingest/locations',
        );
        final seqs = [
          for (final s in (jsonDecode(ingest.body)['samples'] as List))
            (s as Map)['sequence'],
        ];
        expect(seqs, [1, 2]);
        expect(c.log.any((l) => l.message.contains('Entró en «Casa»')), isTrue);
        for (final line in c.log) {
          expect(line.message, isNot(contains('8.98')));
          expect(line.message, isNot(contains(_token)));
        }
      },
    );

    test('detener la compartición descarta la cola y no la envía', () async {
      await c.connect('https://geo.test', _token);
      await c.createSession();
      await c.startSharing();
      fixes.add(_fix(const GeoPoint(1, 1), 1));
      await pumpEventQueue();
      await c.stopSharing();
      await c.flush();
      expect(
        server.requests.where((r) => r.url.path == '/v1/ingest/locations'),
        isEmpty,
      );
      final last = server.requests.lastWhere((r) => r.method == 'PUT');
      expect(jsonDecode(last.body), {'sharing': false});
    });

    test('fallo de ingesta cuenta las muestras como descartadas', () async {
      await c.connect('https://geo.test', _token);
      await c.createSession();
      await c.startSharing();
      server.ingestStatus = 401;
      fixes.add(_fix(const GeoPoint(1, 1), 1));
      await pumpEventQueue();
      await c.flush();
      expect(c.dropped, 1);
      expect(c.accepted, 0);
    });
  });

  test('SequenceAllocator nunca repite tras reabrir', () async {
    final kv = MemoryStore();
    final a = await SequenceAllocator.open(kv, block: 3);
    final first = [
      await a.next(),
      await a.next(),
      await a.next(),
      await a.next(),
    ];
    expect(first, [1, 2, 3, 4]);
    final b = await SequenceAllocator.open(kv, block: 3);
    expect(await b.next(), greaterThan(4));
  });
}
