# 18 — Estrategia de pruebas y CI

**ID:** GS-TST-001 · **Versión:** 0.2.0

## 1. Principios
1. Cada requisito MUST tiene al menos una prueba automatizada que lo cubre (salvo los marcados "manual"), con su ID en el nombre o en un comentario `covers:`.
2. Las pruebas usan dependencias reales (PostgreSQL, Redis/Valkey) en integración; nada de mocks de base de datos.
3. Los contratos (`contracts/`) son la fuente de los casos: vectores y escenarios se consumen tal cual, no se copian.
4. Ninguna prueba depende de servicios externos inestables, de coordenadas reales de personas ni del reloj de pared sin control (usar un `Clock` inyectable).
5. Una prueba intermitente se trata como defecto: se arregla o se pone en cuarentena con issue y fecha límite (≤ 7 días).

## 2. Pirámide

| Nivel | Qué | Dónde | Cuándo corre |
|---|---|---|---|
| Unitarias | Dominio, validación, máquinas de estado, codecs | Cada crate (`#[cfg(test)]`), SDK `test/` | Cada push |
| Propiedades | Secuencias/CAS, validación en bordes, máquinas de estado, coalescencia | `gs-domain`, `gs-stream` (`proptest`) | Cada push |
| Vectores | `contracts/test-vectors/*` | `gs-domain`, `gs-stream`, `gs-api`, SDK | Cada push |
| Integración | Repositorios, servicios, jobs con PG/Redis reales | `gs-store`, `gs-ephemeral`, `gs-bus`, `gs-service`, `gs-worker` | Cada push |
| Contrato | Conformidad OpenAPI, matriz de autorización, Protobuf | `gs-api`, `gs-stream`, `tools/` | Cada push |
| Escenarios E2E | `contracts/scenarios/*` | `gs-testkit` runner | Cada push (rápidos); nocturno (`slow`) |
| Multi-nodo | Escenarios con 2 instancias + Redis/Valkey | `gs-testkit` | Cada PR desde F6 |
| Carga | Perfiles §8 | `apps/gs-loadgen` | Bajo demanda y antes de cada release desde F6 |
| Caos | Caída de Redis/Valkey, de una instancia, de PG | `gs-testkit` + docker | Nocturno desde F6 |
| Fuzzing | Decodificadores y validadores | `fuzz/` (`cargo-fuzz`) | Nocturno desde F10 |
| Dispositivos reales | Checklist manual §9 | — | Cada release desde F5 |

## 3. Unitarias y propiedades
- `gs-domain`: todas las invariantes `INV-*` (`05` §11); validación de muestras con los vectores; transiciones de sesión, membresía, compartición; normalización de códigos de invitación; IDs.
- Propiedades obligatorias:
  - **Secuencia**: para cualquier secuencia de publicaciones (con duplicados, permutaciones, reconexiones simuladas), el conjunto aceptado es estrictamente creciente y coincide con un modelo de referencia simple.
  - **Validación**: valores generados en los bordes ± ε producen exactamente las fronteras de `09` §3.
  - **Coalescencia**: para cualquier intercalado de updates y lecturas lentas, el último update de cada miembro siempre se entrega y la memoria no supera el límite.
  - **Capacidades**: ninguna secuencia de operaciones de miembros con `manage` produce un conjunto de capacidades mayor que el del actor (INV-MEM-2).

## 4. Integración
- Contenedores efímeros por suite (`testcontainers`) o servicios de CI; base de datos migrada desde cero en cada suite.
- `gs-store`: cada repositorio, restricciones del esquema (`11` §2) probadas con inserciones inválidas, patrones transaccionales de `11` §2.1 con concurrencia (p. ej. 20 canjes simultáneos de una invitación de 5 usos → exactamente 5 éxitos).
- `gs-ephemeral` y `gs-bus`: la misma suite se ejecuta contra la implementación en memoria y contra Redis/Valkey (pruebas parametrizadas) para garantizar semántica idéntica.

## 5. Contrato
- **OpenAPI**: en las pruebas de integración de `gs-api`, toda respuesta (incluidos errores) se valida contra el esquema de su operación y código (validador JSON Schema 2020-12 sobre los `components/schemas`). Toda petición de prueba también se valida contra el esquema antes de enviarse.
- **Cobertura de operaciones**: un check falla si una operación de OpenAPI con `x-gs-phase` ≤ fase actual no tiene al menos una prueba de éxito y una de error.
- **Matriz de autorización**: generada a partir de `x-gs-auth`/`x-gs-capability` (`06` §8.2, §10). Se ejecuta completa contra el servidor.
- **Protobuf**: `buf lint`; desde M2, `buf breaking`. Prueba de ida y vuelta JSON → binario → JSON para cada mensaje de `contracts/test-vectors/stream_messages.json` (forma canónica).
- **SDK**: el código Dart generado coincide con los `.proto`; mapeo exhaustivo de `ErrorCode`.

## 6. Escenarios
- Runner en `gs-testkit` que implementa el DSL de `contracts/README.md` §5.
- Cada escenario se ejecuta: (a) nodo único con backend `memory`; (b) nodo único con Redis/Valkey; (c) desde F6, dos instancias con actores repartidos (publicador y suscriptor en instancias distintas); (d) con ambos subprotocolos (`gs.v1.json` y `gs.v1.proto`).
- Las métricas medidas por los escenarios (`metric`) se publican en el resumen de CI (por ejemplo `revocation_propagation_ms` de SCN-002).
- Toda nueva capacidad funcional relevante añade un escenario (`SCN-NNN` siguiente libre).

## 7. Seguridad
- Matriz de autorización completa (REST y stream) y aislamiento de tenants (NFR-SEC-004).
- **Captura de logs**: la suite de escenarios se ejecuta con un suscriptor de `tracing` que guarda todo; un verificador busca: patrones decimales de coordenadas usados en las pruebas (valores de prueba conocidos), `gsk_`, `gst_`, `whsec_`, `eyJ` (JWT), códigos de invitación generados. Cero coincidencias (FR-PRV-004, NFR-SEC-003).
- **Métricas**: tras la suite, `/metrics` no contiene IDs ni valores prohibidos (NFR-OBS-002).
- **SSRF**: destinos prohibidos por IP literal y por DNS (servidor DNS de prueba), redirecciones no seguidas.
- **Revocación**: medición del plazo (SCN-002) y prueba con bus que descarta mensajes → peor caso ≤ `GS_AUTHZ_CACHE_TTL_MS` + 1 s.
- **Límites de abuso**: cada límite de `10` §5.4 tiene una prueba que lo excede.
- **Fuzzing** (F10): objetivos `fuzz_client_message_proto`, `fuzz_client_message_json`, `fuzz_validate_sample`, `fuzz_invitation_code`, `fuzz_problem_json_roundtrip`; ≥ 10 min por objetivo cada noche; corpus semilla desde los vectores.

## 8. Rendimiento

### 8.1 Perfiles de carga (`gs-loadgen`)

| Perfil | Conexiones | Forma | Publicadores | Frecuencia | Entrada aprox. | Entregas aprox. |
|---|---|---|---|---|---|---|
| **P-S** | 1 000 | 250 sesiones `MANY_TO_MANY` × 4 miembros; 2 publican; los 4 suscriben | 500 | 1 cada 3 s | 167 muestras/s | 667/s |
| **P-M** | 10 000 | 2 500 sesiones × 4 miembros; 2 publican; los 4 suscriben | 5 000 | 1 cada 3 s | 1 667 muestras/s | 6 667/s |
| **P-L** | 30 000 en 3 nodos | Como P-M ×3, con publicadores y suscriptores repartidos entre nodos | 15 000 | 1 cada 3 s | 5 000 muestras/s | 20 000/s |
| **P-FLEET** | 2 001 | 1 sesión de aplicación, 2 000 publicadores sin `read` + 1 consumidor de aplicación | 2 000 | 1 cada 5 s | 400 muestras/s | 400/s |
| **P-STORM** | 10 000 | P-M; se cortan todas las conexiones y se reconectan con la política del SDK | — | — | — | — |

### 8.2 Medición e informe
- Latencias desde las métricas del servidor (`gs_stream_delivery_latency_seconds`) y desde el generador (marca de tiempo en `captured_at` vs recepción en el suscriptor, con relojes sincronizados en el mismo host o región).
- Duración mínima: 30 min por perfil (10 min de calentamiento excluidos).
- Informe en `docs/benchmarks/AAAA-MM-DD-<perfil>.md` con: versión/commit, hardware, configuración, resultados (p50/p95/p99, CPU, memoria, errores) y comparación con los NFR. Los objetivos se marcan como **objetivo** y los resultados como **medido**.

## 9. Checklist de dispositivos reales (manual, cada release desde F5)

Dispositivos mínimos (NFR-MOB-001): Android sin capa del fabricante, Android de un fabricante con gestión agresiva de batería, iPhone con cada una de las dos versiones mayores de iOS soportadas (desde F7).

Por dispositivo:
- [ ] Permisos: denegar, denegar permanentemente, conceder aproximada, conceder precisa, conceder "siempre".
- [ ] Servicio de ubicación apagado antes y durante la compartición.
- [ ] Compartir 30 min en primer plano (`balanced`): continuidad, consumo de batería anotado.
- [ ] Compartir 30 min con la app en segundo plano (`continueInBackground`): continuidad, notificación/indicador visible.
- [ ] Pantalla bloqueada 15 min.
- [ ] Modo avión 2 min y recuperación: al volver, como máximo una muestra antigua.
- [ ] Cambio Wi-Fi ↔ datos móviles durante la compartición.
- [ ] Revocación desde el backend: la app deja de recibir y lo muestra.
- [ ] Detener compartición sin conexión y reconectar: la detención llega al servidor.
- [ ] Reinicio de la app: no se repiten secuencias.
- [ ] Forzar cierre de la app mientras comparte: comportamiento documentado.

Resultado en `docs/device-tests/AAAA-MM-DD-<release>.md`.

## 10. CI

### 10.1 Etapas (se crean en F0 y crecen por fase)
1. **Formato y lint**: `cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `dart format --set-exit-if-changed`, `dart analyze --fatal-infos` / `flutter analyze`.
2. **Contratos**: `buf lint`, Redocly lint, JSON válido en `contracts/`, (desde M2) `buf breaking` y `oasdiff`.
3. **Consistencia documental** (`tools/`):
   - catálogo de errores idéntico en `common.proto`, OpenAPI (`ErrorCode`), `09` §6 y el mapeo del SDK;
   - toda clave `GS_*` y de política citada en `docs/` existe en `17`;
   - todo ID de requisito citado existe en `03`; todo requisito aparece en `TRACEABILITY.md`;
   - todo `operationId` de OpenAPI aparece en la matriz de `06` §8.2 con los mismos scopes;
   - enlaces relativos de Markdown válidos;
   - reglas de dependencias entre crates (`04` §4.1).
4. **Pruebas**: unitarias, propiedades, vectores, integración (PG + Valkey/Redis), contrato, escenarios rápidos.
5. **Seguridad**: `cargo deny check` (advisories, licencias, orígenes), `cargo audit`, búsqueda de secretos en el diff.
6. **Build**: binario de release, imagen de contenedor con SBOM, (SDK) build Android de la app de ejemplo; desde F7 build iOS.
7. **Cobertura**: informe con umbrales de NFR-MAINT-002.

### 10.2 Checks obligatorios para fusionar
Todas las etapas 1–5 y 7 en verde. La etapa 6 obligatoria en la rama principal y en tags.

### 10.3 Suites nocturnas
Escenarios `slow`, multi-nodo completo, caos, fuzzing (desde F10), carga P-S (desde F6).

## 11. Convenciones de nombres y trazabilidad
- Rust: `fn fr_ses_001_creates_session_with_policy_defaults()` o atributo/comentario `// covers: FR-SES-001`.
- Dart: `test('FR-SDK-005 never repeats sequences after restart', …)`.
- Un script de `tools/` extrae los `covers`/IDs de las pruebas y regenera la columna de pruebas de `TRACEABILITY.md`; CI falla si un requisito MUST de una fase cerrada no tiene pruebas.
