# GeoStream Tester (`apps/flutter_app`)

App Flutter (Android/iOS) **de pruebas** para validar, en un teléfono real, que la ubicación precisa llega al
servidor GeoStream desplegado en Railway (`https://geostream.gravitalcloud.com`). No es el SDK
(`sdks/flutter/`, fase F5) ni una app para usuarios finales.

## Qué hace

| Pestaña | Función | API usada (todas ya implementadas en el servidor) |
|---|---|---|
| Conexión | URL del servidor, token de dispositivo, crear/usar sesión, interruptor «Compartir» (apagado por defecto), contadores aceptadas/rechazadas/descartadas y registro | `GET /health/ready`, `POST /v1/sessions`, `PUT /v1/sessions/{id}/members/me/sharing` |
| Mapa | Mapa OpenStreetMap (MapLibre) con mi posición y su círculo de precisión, los miembros del snapshot y las geocercas coloreadas por estado | `POST /v1/ingest/locations`, `GET /v1/sessions/{id}/snapshot` |
| Geocercas | Editor sobre el mapa: círculo (toca el centro, ajusta el radio) o polígono (toca los vértices). Valida con las reglas de `docs/14_GEOFENCING.md` §3 y copia el `CreateGeofenceRequest` del contrato | Ninguna todavía |

**Geocercas: vista previa local.** `POST /v1/geofences` es de la fase F11 y el servidor aún no la implementa. Las
geocercas se guardan en el teléfono y se evalúan con la histéresis de `14` §4 (defaults de `17`:
`confirm_samples=2`, `exit_buffer_m=20`, `max_accuracy_m=100`) sobre las muestras que el servidor **aceptó**.
La transición oficial la decide siempre el servidor (`14` §5.1); la app no envía eventos de geocerca.

## Reglas que cumple

- El token **no se guarda**: vive en memoria. No hay API key en la app: el token lo emite un backend con
  `POST /v1/tokens` (`docs/06_IDENTITY_AUTH.md` §4.1).
- Compartir está apagado por defecto y solo lo enciende el usuario. Al apagarlo, las muestras pendientes se
  **descartan** y se envía `sharing=false`.
- El registro y los errores nunca muestran coordenadas ni credenciales (prueba en `test/api_and_controller_test.dart`).
- Conversión de muestras según `docs/12_FLUTTER_SDK.md` §11 y secuencia por bloques persistida (`12` §9).
- Mapa: teselas del servidor comunitario de OSM **solo para pruebas** (`docs/13_MAPS_PROVIDERS.md` §2), con la
  atribución «© OpenStreetMap contributors» siempre visible. Para cualquier uso continuado, compila con teselas
  propias o de un proveedor: `--dart-define=GS_TILE_URL=https://…/{z}/{x}/{y}.png`.
- Dependencias de la lista aprobada (`docs/19_ENGINEERING_STANDARDS.md` §6.3): `http`, `geolocator`,
  `shared_preferences`, `maplibre_gl`, `flutter_lints`, `flutter_test`.

## Obtener un token de dispositivo

Una vez, el operador da de alta el tenant, la aplicación y su API key desde una shell del servicio
(`railway ssh`, ver `deploy/README.md`):

```sh
geostream admin tenant create --name pruebas
geostream admin app create --tenant ten_… --name tester \
  --device-scopes sessions:create,sharing:write,ingest:write,stream:connect
geostream admin key create --app app_… --scopes tokens:mint --label tester
```

La key `gsk_…` se muestra una sola vez: guárdala en un gestor de secretos. Luego, desde una máquina de confianza
(nunca desde el teléfono), emite un token de prueba de hasta 1 h:

```sh
curl -sS https://geostream.gravitalcloud.com/v1/tokens \
  -H "authorization: Bearer $GS_API_KEY" -H 'content-type: application/json' \
  -d "{\"external_subject_id\":\"tester-1\",\"installation_id\":\"$(python3 -c 'import uuid;print(uuid.uuid4())')\",
       \"platform\":\"ANDROID\",\"scopes\":[\"sessions:create\",\"sharing:write\",\"ingest:write\"],\"ttl_seconds\":3600}"
```

Pega `access_token` en la pestaña Conexión. Para ver a otro teléfono en el mapa, crea la sesión desde tu backend
con los dos subjects como miembros con `read` y usa «ID de sesión existente».

## Ejecutar y probar

```sh
cd apps/flutter_app
flutter pub get
flutter analyze
flutter test
flutter run            # con un teléfono conectado
```

Probado: `flutter analyze` y `flutter test` (Flutter 3.47.9). La compilación nativa (APK/IPA) necesita el SDK de
Android o Xcode.

## Límites conocidos

- Solo primer plano: no hay servicio en segundo plano (eso es T-F5-10 del SDK).
- Ingesta por REST cada 2 s, no por el stream WebSocket.
- Sin reintentos: un lote fallido se cuenta como «descartado» para que el fallo se vea.
