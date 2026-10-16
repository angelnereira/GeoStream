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
│   ├── stream_messages.json
│   ├── tracking_estimates.json  # pista E: TRK-*
│   ├── sampling_policy.json      # pista E: SMP-*
│   ├── pool_queries.json         # pista E: POOL-*
│   ├── mobility_tools.json       # pista E: ETA-*, PRD, ASG, ANA-*
│   └── edge_reports.json         # pista E: EDGE-*
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

### 4.7 `tracking_estimates.json` (pista E, `TRK-*`)
Filtro de trayectoria del servidor (`docs/22_TRACKING_ENGINE.md §2`): puerta de plausibilidad, filtro de Kalman, reinicios, odómetro y paradas online. Incluye `constants` (valores no configurables de `17` §2.11), `tolerances` de comparación numérica y conjuntos de banderas con nombre.

### 4.8 `sampling_policy.json` (pista E, `SMP-*`)
Muestreo adaptativo y publicación por cambio significativo (`22` §3). Incluye los rangos de acotamiento de `SamplingPolicy` (T-30), los casos de cola sin conexión cifrada y los de predicción visual.

### 4.9 `pool_queries.json` (pista E, `POOL-*`)
Pools de disponibilidad (`docs/23_MOBILITY.md`): consultas `nearby`/`rings`/`density`, invariantes `INV-POOL-1` e `INV-PRS-2`, y casos de validación.

### 4.10 `mobility_tools.json` (pista E, `ETA-*`, `PRD`, `ASG`, `ANA-*`)
Herramientas de movilidad, Provider Gateway BYOK, capa de mapa aprendida y k-anonimato. Los casos `*_DEG-*` fijan la regla más importante del conjunto: **nunca degradar en silencio**.

### 4.11 `edge_reports.json` (pista E, `EDGE-*`)
Red GeoStream Beacon: rotación de `EID`, reportes cifrados que el servidor no puede leer, consultas del dueño, registro de tags, DULT e ingesta LPWAN.

**Doble uso de los vectores de la pista E.** Los valores sintéticos de `GEO-*`, `TRK-*`, `SMP-*`, `POOL-*`, `ETA-*`, `ASG`, `PRD` y `EDGE-*` son **también** valores prohibidos en logs, métricas, trazas y respuestas de error. El verificador de redacción (`docs/18_TESTING_STRATEGY.md` §7.1) los carga de aquí; por eso no se pueden usar números reales en los vectores.

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
  "note": "texto libre que explica la intención del escenario",
  "actors": { "<nombre>": { … } },
  "steps": [ { … } ]
}
```

### 5.1.1 Escenarios de la pista E

| ID | Qué fija |
|---|---|
| SCN-014 | La estimación acompaña a la muestra sin sustituirla (P-E3) |
| SCN-015 | Un conflicto del motor no impide aceptar la muestra |
| SCN-016 | En un `POOL` los conductores no se ven entre sí (`INV-POOL-1`) |
| SCN-017 | `density` no revela quién está dónde (T-25) |
| SCN-018 | Detener la compartición borra el índice de forma síncrona |
| SCN-019 | Retirar `HISTORY` detiene las escrituras de inmediato |
| SCN-020 | El backend no puede conceder consentimiento (`INV-SHR-5`) |
| SCN-021 | k-anonimato: lo que está por debajo de *k* no aparece |
| SCN-022 | El proveedor recibe solo coordenadas (T-29) |
| SCN-023 | Proveedor caído → 503, nunca estimación silenciosa |
| SCN-024 | El servidor no puede descifrar el reporte Beacon |
| SCN-025 | Estado `SEPARATED` y protección DULT |
| SCN-026 | Compatibilidad hacia atrás (NFR-COMPAT-003) |

SCN-014..SCN-026 usan `op` adicionales de aserción (`assert.*`) que el runner debe soportar antes de que la fase correspondiente declare su escenario verde.

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
| `assert.metric` | `what`, `expect: { increased? }` | Verificar un contador Prometheus tras el paso |
| `assert.db_row` | `table`, `expect` | Inspeccionar una fila real de PostgreSQL (solo en pruebas) |
| `assert.schema` | `what: "no_column_for_eik"`, `expect` | Inspeccionar el esquema (solo en pruebas) |
| `assert.provider_request` | `for_port`, `expect` | Inspeccionar lo que el proveedor de pruebas recibió (solo en pruebas) |
| `assert.server_cannot_decrypt` / `assert.client_can_decrypt` | `what` / `with_key` | Verificar la frontera de cifrado |
| `assert.dult` | `tag_id`, `params`, `expect` | Evaluar los umbrales DULT con el reloj controlado |
| `assert.eid_rotation` | `tag_id`, `state_before`, `state_after`, `period_s`, `period_after_s` | Verificar la rotación del identificador efímero |
| `assert.no_secret_leak` | `secret`, `in: [...]` | Buscar un secreto en logs, métricas, trazas y respuestas |
| `assert.received_message_fields`, `assert.server_hello`, `assert.no_error_for_unknown_enum` | `expect` | Verificar tolerancias de compatibilidad del cliente |
| `sleep` | `ms` | Esperar (evitar salvo necesidad) |

Campos comunes: `note` (texto libre), `mark` (registra el instante del paso), `measure_from` + `metric` (mide la latencia desde un `mark` y la reporta).

### 5.4 Plantillas
- `{{variable}}`: valor capturado con `capture` (JSONPath; las expresiones de filtro `[?(@.campo=='valor')]` deben estar soportadas por el runner).
- `{{now}}`, `{{now-1s}}`, `{{now+500ms}}`: instantes RFC 3339 relativos al reloj del runner, corregido con el desfase respecto al servidor medido en el hello.
- `{{<actor>.token}}`: token vigente del actor.

### 5.5 Coincidencia parcial
Un objeto esperado coincide si cada clave presente coincide recursivamente; los arrays esperados deben coincidir elemento a elemento en longitud y orden salvo que se use `json_count`. `null` esperado exige `null` o ausencia.
