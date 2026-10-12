# Contratos máquina de GeoStream

Los archivos de esta carpeta son **normativos**. El código se genera o se prueba contra ellos; nunca al revés.

```text
contracts/
├── proto/                       # Protocolo de stream (Protobuf, buf)
│   ├── buf.yaml
│   └── geostream/v1/{common,location,session,stream}.proto
├── openapi/
│   ├── geostream.v1.yaml        # API REST + webhooks (OpenAPI 3.1)
│   ├── redocly.yaml             # reglas de lint
│   └── .redocly.lint-ignore.yaml
├── test-vectors/                # Casos de entrada → salida esperada
│   ├── location_samples.json
│   ├── ids.json
│   ├── invitation_codes.json
│   ├── webhook_signature.json
│   ├── token_claims.json
│   └── stream_messages.json
└── scenarios/                   # Escenarios extremo a extremo (SCN-NNN)
```

## 1. Comandos de verificación (CI desde F0)

```bash
# Protobuf
(cd contracts/proto && buf lint)
(cd contracts/proto && buf breaking --against '../../.git#branch=main,subdir=contracts/proto')   # obligatorio desde M2

# OpenAPI
npx @redocly/cli lint contracts/openapi/geostream.v1.yaml --config contracts/openapi/redocly.yaml
oasdiff breaking <openapi de main> contracts/openapi/geostream.v1.yaml --fail-on ERR             # obligatorio desde M2

# Vectores y escenarios: JSON válido y consumidos por las suites (ver docs/18_TESTING_STRATEGY.md)
```

Las versiones exactas de `buf`, `@redocly/cli` y `oasdiff` se fijan en F0 (`docs/19_ENGINEERING_STANDARDS.md` §6).

## 2. Generación de código

| Destino | Herramienta | Ubicación | Notas |
|---|---|---|---|
| Rust | `prost-build` + `pbjson-build` en `build.rs` de `gs-protocol` | `crates/gs-protocol/src/gen/` (no versionado; generado en build) | `protoc` vía `protoc-bin-vendored` para no depender del sistema. `pbjson` aporta el mapeo JSON canónico y debe configurarse para ignorar campos desconocidos. |
| Dart | `protoc` + `protoc_plugin` | `sdks/flutter/geostream/lib/src/gen/` (**versionado**, requerido por pub) | Regenerar con `tools/gen_dart_proto.sh` (F5). CI verifica que lo versionado coincide con lo generado. |
| OpenAPI → Rust | No se genera código | — | Los tipos REST se escriben a mano en `gs-api` y se validan contra el esquema en las pruebas de conformidad. |
| OpenAPI → Dart | No se genera código | — | El cliente REST del SDK es manual y pequeño; se valida con pruebas de contrato. |

## 3. Correspondencias entre representaciones

| Concepto | Protobuf | JSON de stream (`gs.v1.json`) | REST |
|---|---|---|---|
| Nombres de campo | `snake_case` | salida `lowerCamelCase`, entrada ambos | `snake_case` |
| Enum | `SESSION_MODE_ONE_TO_ONE` | `"SESSION_MODE_ONE_TO_ONE"` | `"ONE_TO_ONE"` |
| `uint64 sequence` | varint | `"42"` (cadena) | `42` (número, ≤ 2^53−1) |
| Tiempo | `google.protobuf.Timestamp` | `"2026-10-11T21:04:05.123Z"` | igual |
| Código de error | `ERROR_CODE_SESSION_FULL` | `"ERROR_CODE_SESSION_FULL"` | `"SESSION_FULL"` |

## 4. Formato de los vectores de prueba

### 4.1 `location_samples.json`
- `server_now`: el `now` del servidor para todos los casos.
- `default_policy`, `default_last_sequence`: estado inicial de cada caso (la secuencia previa aceptada para el par dispositivo/sesión). `last_sequence: null` en un caso = sin muestras previas.
- `base_sample`: muestra válida en formato REST (snake_case, enums cortos, `sequence` numérico). Los no finitos se escriben `"NaN"`, `"Infinity"`, `"-Infinity"`.
- `cases[]`: cada caso aplica `patch` sobre una copia de `base_sample`. Las claves del patch son rutas con puntos; el valor `null` **elimina** el campo (en proto3 equivale al valor por defecto). Un caso puede sobrescribir `policy` (fusión superficial) y `last_sequence`.
- `expect`: `status` y, si es `REJECTED`, `error_code` y `field` exactos.
- `batches[]`: lotes procesados en una sola llamada; `expect.request_error` (lote rechazado completo) o `expect.results` (estado por muestra), más `final_last_sequence` y `snapshot_sequence` (secuencia del snapshot tras el lote, `null` = sin snapshot nuevo).
- Consumidores obligatorios: pruebas unitarias de `gs-domain` (validación), pruebas de integración del stream, pruebas de la ingesta HTTP (F8) y pruebas del validador local del SDK.

### 4.2 `ids.json`
Valores válidos e inválidos por tipo de ID, `external_subject_id` e `installation_id`.

### 4.3 `invitation_codes.json`
Normalización de códigos y hash esperado.

### 4.4 `webhook_signature.json`
Secreto, cabeceras, cuerpo exacto y firma esperada. El cuerpo se firma **byte a byte** tal cual aparece.

### 4.5 `token_claims.json`
Forma exacta de cabecera y claims; `forbidden_extra_claims: true` exige que no haya otros claims.

### 4.6 `stream_messages.json`
Un ejemplo completo y válido de cada mensaje de `ClientMessage`/`ServerMessage` en JSON canónico proto3 (sin campos con valor por defecto). Uso: prueba de ida y vuelta JSON → binario → JSON (T-F0-04), codec del SDK y simulador de transporte. Verificado con `buf convert`.

## 5. Escenarios (`scenarios/*.json`)

Cada archivo describe un flujo extremo a extremo que el runner de `gs-testkit` ejecuta contra un servidor real (PostgreSQL + almacén efímero; en F6 también con dos instancias y Redis/Valkey).

### 5.1 Estructura

```jsonc
{
  "id": "SCN-002",
  "title": "…",
  "requirements": ["FR-MEM-004", "NFR-SEC-002"],   // IDs cubiertos
  "slow": false,                                  // true = solo en la suite lenta/nocturna
  "config": { "GS_…": 123 },                       // sobrescrituras de configuración del servidor
  "actors": { "<nombre>": { … } },
  "steps": [ { … } ]
}
```

### 5.2 Actores
- `{"kind": "application", "scopes": [...], "tenant": "default"|"<otro>"}` → el runner crea (o reutiliza) el tenant, una aplicación y una API key con esos scopes.
- `{"kind": "device", "external_subject_id": "...", "platform": "PLATFORM_ANDROID", "tenant": "...", "scopes": [...]?, "token_ttl_seconds": N?}` → el runner emite un token para un `installation_id` aleatorio fijo por actor usando una aplicación de arranque del tenant. `{{<actor>.token}}` referencia el token vigente.
- Si no se indica `tenant`, es `"default"`.

### 5.3 Operaciones (`op`)

| `op` | Campos | Semántica |
|---|---|---|
| `http` | `method`, `path`, `body?`, `headers?`, `expect`, `capture?` | Petición REST autenticada como el actor. `expect.status` exacto; `expect.json` = coincidencia parcial; `expect.json_count` = `{ruta: n}` |
| `stream.connect` | `connection?` (default `"main"`), `hello?` (campos que sobrescriben el `ClientHello`), `expect: "ready"`, `expect_hello?` | Ticket + upgrade (`gs.v1.json` por defecto; la suite binaria repite con `gs.v1.proto`) + hello |
| `stream.send` | `connection?`, `message` (cuerpo de `ClientMessage` en JSON proto3 con claves `snake_case`), `expect_reply?` | El runner asigna `request_id`. `expect_reply` = coincidencia parcial sobre la respuesta con el mismo `request_id` (timeout 2000 ms) |
| `stream.expect` | `connection?`, `message` (nombre del campo del `oneof`), `match?`, `within_ms` | Esperar un mensaje no solicitado que coincida |
| `stream.expect_none` | `connection?`, `message`, `match?`, `for_ms` | Verificar que no llega |
| `stream.expect_close` | `connection?`, `code`, `within_ms` | Esperar cierre con ese código |
| `stream.close` | `connection?` | Cerrar con 1000 |
| `token.refresh` | — | Emitir un token nuevo para el mismo dispositivo del actor |
| `assert.ephemeral` | `what: "snapshot"`, `session`, `member`, `exists` | Inspeccionar el almacén efímero directamente (solo en pruebas) |
| `sleep` | `ms` | Esperar (evitar salvo necesidad) |

Campos comunes: `note` (texto libre), `mark` (registra el instante del paso), `measure_from` + `metric` (mide la latencia desde un `mark` y la reporta).

### 5.4 Plantillas
- `{{variable}}`: valor capturado con `capture` (JSONPath; las expresiones de filtro `[?(@.campo=='valor')]` deben estar soportadas por el runner).
- `{{now}}`, `{{now-1s}}`, `{{now+500ms}}`: instantes RFC 3339 relativos al reloj del runner, corregido con el desfase respecto al servidor medido en el hello.
- `{{<actor>.token}}`: token vigente del actor.

### 5.5 Coincidencia parcial
Un objeto esperado coincide si cada clave presente coincide recursivamente; los arrays esperados deben coincidir elemento a elemento en longitud y orden salvo que se use `json_count`. `null` esperado exige `null` o ausencia.
