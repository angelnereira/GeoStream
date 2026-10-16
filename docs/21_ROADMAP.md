# 21 — Hoja de ruta de implementación

**ID:** GS-RMP-001 · **Versión:** 0.3.0**
De repositorio vacío a **GeoStream v1.0 GA** (F0–F12), pistas posteriores (F13–F16)
y la **pista E** de motor de ubicación y movilidad (E0–E9).

## 1. Reglas
- El roadmap define **secuencia**, no comportamiento: el comportamiento está en los documentos citados.
- Cada tarea `T-Fx-NN` es una unidad de PR. Columnas: requisitos que cubre, documentos que hay que leer, dependencias.
- Una fase se cierra cuando se cumplen **todos** sus criterios de salida y queda registrado en `PROGRESS.md` con evidencia (enlaces a CI, informes).
- Estimaciones orientativas para un desarrollador con agentes de código; no son compromisos.
- "Adelantable" = puede empezar antes de cerrar la fase anterior si sus dependencias están cerradas.
- **La pista E no se mezcla con la línea base.** Solo un PR toca `contracts/` a la vez, y E0 (documentación, contratos y ADR) no escribe código de producción (`20` §4).

## 2. Mapa de fases e hitos

```mermaid
flowchart LR
  F0[F0 Bootstrap y CI] --> F1[F1 Dominio]
  F1 --> F2[F2 Persistencia e identidad]
  F2 --> F3[F3 Plano de control REST]
  F3 --> F4[F4 Stream nodo único<br/>M1]
  F4 --> F5[F5 SDK Flutter Android<br/>M2: contrato v1 congelado]
  F4 --> F6[F6 Multi-instancia y rendimiento]
  F5 --> F7[F7 iOS y endurecimiento móvil<br/>M3: Beta privada]
  F6 --> F7
  F3 --> F8[F8 Integraciones]
  F6 --> F8
  F5 --> F9[F9 Mapas<br/>M4: integrable]
  F8 --> F9
  F7 --> F10[F10 Privacidad y seguridad]
  F9 --> F10
  F6 --> F11[F11 Geocercas]
  F10 --> F12[F12 Producción<br/>M5: v1.0 GA]
  F11 --> F12
  M5 -.->|v1.1 a v1.5| E0["Pista E: motor de ubicación<br/>y movilidad (E0–E9)"]
```

| Hito | Al cerrar | Significado | Release |
|---|---|---|---|
| M0 | F0 | Repositorio con CI y contratos verificados | — |
| M1 | F4 | Backend funcional en nodo único con todos los escenarios | `server-v0.1.0-alpha.1` |
| M2 | F5 | Dos teléfonos Android comparten; **contrato v1 congelado** | `sdk-flutter-v0.1.0-alpha.1` |
| M3 | F7 | Beta privada: Android + iOS, multi-nodo, benchmarks | `server-v0.5.0-beta.1`, `sdk-flutter-v0.5.0-beta.1` |
| M4 | F9 | Integrable: webhooks, consumidores de servidor, ingesta, mapas | `-rc.1` |
| M5 | F12 | **v1.0 GA** | `server-v1.0.0`, `sdk-flutter-v1.0.0` |
| M6..M11 | E2, E3, E5, E7, E8, E9 | Capacidades del motor, por versión | `server-v1.1.0` … `server-v1.5.0` |

Estimación total F0–F12: 32–45 semanas. Pista E1–E9: 26–38 semanas adicionales.

---

## F0 — Bootstrap del repositorio y CI (1 semana) → M0

**Objetivo:** estructura, toolchain, CI y verificación de contratos antes de escribir lógica.

| ID | Tarea y entregable | Requisitos | Leer | Depende |
|---|---|---|---|---|
| T-F0-01 | Workspace Cargo con la estructura de `04` §4 (crates vacíos con `#![forbid(unsafe_code)]`), `rust-toolchain.toml` (stable vigente), edición 2024; revisar `.editorconfig` y `.gitignore` existentes | FR-OPS-001 | 04 §4, 19 §3 | — |
| T-F0-02 | Fijar dependencias aprobadas en `Cargo.lock`; `deny.toml` con licencias de `19` §6.1; `cargo deny` y `cargo audit` en CI | NFR-SEC-006 | 19 §6 | T-F0-01 |
| T-F0-03 | Binario `geostream` con `version` (versión + commit), `serve` (listener mínimo con `/health/live`), `migrate` y `admin` como stubs que devuelven "no implementado" | FR-OPS-001 | 04 §5 | T-F0-01 |
| T-F0-04 | `gs-protocol`: `build.rs` con `prost-build` + `pbjson-build` + `protoc-bin-vendored`; prueba de ida y vuelta JSON → binario → JSON con todos los mensajes de `contracts/test-vectors/stream_messages.json` | — | 08, contracts/README §2 | T-F0-01 |
| T-F0-05 | CI etapas 1, 2, 3 y 5 (`18` §10.1): formato, lint, `buf lint`, Redocly lint, JSON válido, checks de consistencia documental en `tools/` (catálogo de errores, claves de configuración, IDs de requisitos, matriz `06` §8.2 vs `x-gs-auth`, enlaces, reglas de crates) | NFR-DOC-001, NFR-SEC-006 | 18 §10, 00 | T-F0-02 |
| T-F0-06 | Proceso de release y changelog (plantilla de release, verificación de `CHANGELOG.md` en PR) | NFR-MAINT-001 | 19 §8 | T-F0-05 |
| T-F0-07 | `deploy/docker-compose.dev.yml` (PostgreSQL + Valkey) y `deploy/.env.example` | — | 16 §10, 17 | T-F0-01 |
| T-F0-08 | Verificar disponibilidad de nombres (crates.io, pub.dev) y registrar en `PROGRESS.md`; recordar ADR-0019 (licencia) pendiente | — | ADR-0019 | — |

**Criterios de salida:** CI verde en `main` con todas las etapas aplicables; `geostream version` funciona; `gs-protocol` compila los contratos; los checks de consistencia pasan sobre la documentación actual.

---

## F1 — Núcleo de dominio (1–2 semanas)

**Objetivo:** reglas de negocio puras y probadas, sin I/O.

| ID | Tarea y entregable | Requisitos | Leer | Depende |
|---|---|---|---|---|
| T-F1-01 | IDs tipados con prefijo y codec; vectores `ids.json` | FR-API-006 (base) | 05 §2 | F0 |
| T-F1-02 | Tipos de ubicación redactados y `validate_sample` puro; vectores `location_samples.json` (`cases`) | FR-LOC-001, FR-LOC-002, FR-LOC-004, FR-LOC-006 | 09 §3, 05 §9 | T-F1-01 |
| T-F1-03 | Modelo de ordenación de secuencias (`batches` de los vectores, sobre un almacén simulado) | FR-LOC-003 | 09 §4 | T-F1-02 |
| T-F1-04 | Sesión, membresía, capacidades, compartición, invitación: estados, transiciones e invariantes `INV-*` | FR-SES-006, FR-SES-007, FR-MEM-003, FR-MEM-007, FR-SHR-001, FR-SHR-002, FR-SHR-004 | 05 | T-F1-01 |
| T-F1-05 | Generación y normalización de códigos de invitación; vectores `invitation_codes.json` | FR-INV-001 (base) | 05 §6 | T-F1-01 |
| T-F1-06 | `gs-config`: carga y validación de configuración y restricciones por entorno; política de tenant con defaults y validación cruzada | FR-OPS-003, FR-ADM-002 | 17 | F0 |
| T-F1-07 | Errores de dominio y mapeo único a `ErrorCode` | FR-API-001 (base) | 09 §6 | T-F1-04 |
| T-F1-08 | Pruebas de propiedades (secuencias, capacidades) y cobertura ≥ 85 % de `gs-domain` | NFR-MAINT-002 | 18 §3 | T-F1-03, T-F1-04 |

**Criterios de salida:** vectores de dominio verdes; invariantes con pruebas nombradas; `gs-domain` sin dependencias de I/O; cobertura cumplida.

---

## F2 — Persistencia, identidad y base operativa (2–3 semanas)

| ID | Tarea y entregable | Requisitos | Leer | Depende |
|---|---|---|---|---|
| T-F2-01 | `migrations/0001_init.sql` exacto a `11` §2; `geostream migrate`; pruebas de restricciones; verificación de versión al arrancar | FR-OPS-004 | 11 | F1 |
| T-F2-02 | `gs-store`: repositorios de tenants, aplicaciones, keys, subjects, dispositivos y auditoría con filtro por tenant | FR-AUTH-001 (persistencia) | 11, 05 §3 | T-F2-01 |
| T-F2-03 | CLI `geostream admin` (tenant, app, key, policy, signing-key) | FR-ADM-001, FR-ADM-002 | 16 §3, 06 §3–§4 | T-F2-02 |
| T-F2-04 | `gs-auth`: API keys (formato, hash, tiempo constante), JWT EdDSA (emisión, verificación en el orden de `06` §4.3, rotación), scopes; vector `token_claims.json` | FR-AUTH-001, FR-AUTH-003, FR-AUTH-004, FR-AUTH-008 | 06 | T-F2-02 |
| T-F2-05 | `gs-ephemeral` y `gs-bus`: traits e implementación en memoria; suite parametrizada preparada para Redis/Valkey | — | 04 §6, 11 §3 | F1 |
| T-F2-06 | `gs-observability`: tipos redactados, métricas base, logs JSON, verificador de captura de logs | FR-PRV-004 (base), NFR-OBS-001, NFR-OBS-002, NFR-SEC-003 | 10 §8, 16 §4 | F1 |
| T-F2-07 | `gs-api` esqueleto: router, `X-Request-Id`, Problem Details, extracción de principal, límites de cuerpo, logs de acceso redactados, `/health/*`, `/metrics` en admin | FR-OPS-002, FR-API-001 (base) | 07 §1–§5, 16 §4 | T-F2-04, T-F2-06 |
| T-F2-08 | `POST /v1/tokens` con upsert de subject/dispositivo e INV-DEV-2 | FR-AUTH-002 | 06 §4, OpenAPI `mintToken` | T-F2-07 |
| T-F2-09 | Configuración de TLS de referencia documentada en `deploy/` (proxy) | NFR-SEC-001 | 16 §2.3 | T-F0-07 |

**Criterios de salida:** tokens emitidos y verificados con todos los casos negativos de `06` §4.3; migraciones idempotentes; salud y métricas operativas; verificador de logs activo.

---

## F3 — Plano de control REST (3–4 semanas)

| ID | Tarea y entregable | Requisitos | Leer | Depende |
|---|---|---|---|---|
| T-F3-01 | Idempotencia con almacenamiento cifrado de respuestas (AES-256-GCM) | FR-API-002 | 07 §6, 10 §5.6 | F2 |
| T-F3-02 | Paginación por cursor | FR-API-003 | 07 §7 | F2 |
| T-F3-03 | Sesiones: crear (dispositivo/aplicación, propietario, miembros iniciales), listar, obtener, actualizar con `If-Match`, cerrar; job de expiración + verificación perezosa | FR-SES-001, FR-SES-002, FR-SES-003, FR-SES-004, FR-SES-005, FR-SES-006 | 05 §4, 07 §9.2, 11 §2.1, §5 | T-F3-01, T-F3-02 |
| T-F3-04 | Miembros: alta, listado, consulta (`me`), cambio de capacidades/rol, revocación, salida, transferencia de propiedad, readmisión, límite de miembros con bloqueo | FR-MEM-001, FR-MEM-002, FR-MEM-003, FR-MEM-004, FR-MEM-005, FR-MEM-006, FR-MEM-007, FR-SES-007 | 05 §5, 07 §9.3 | T-F3-03 |
| T-F3-05 | Compartición: `PUT …/sharing` con efectos (borrado de snapshot, `mver++`, evento de control) | FR-SHR-001, FR-SHR-002, FR-SHR-003, FR-SHR-004 | 05 §7, 04 §7.4 | T-F3-04 |
| T-F3-06 | Invitaciones: crear, listar, revocar, canjear con concurrencia y rate limit | FR-INV-001, FR-INV-002, FR-INV-003 | 05 §6, 07 §9.4 | T-F3-04 |
| T-F3-07 | Subjects y dispositivos: búsqueda, consulta, revocación de tokens, desactivación de dispositivo | FR-AUTH-005 | 06 §7, 07 §9.5 | T-F3-03 |
| T-F3-08 | Deshabilitar/habilitar tenant y aplicación por CLI con invalidación de cachés | FR-ADM-003 | 06 §7 | T-F3-07 |
| T-F3-09 | Auditoría de acciones sensibles | FR-PRV-005, FR-SHR-005 | 10 §8.3 | T-F3-04 |
| T-F3-10 | Rate limits REST e invitaciones | FR-API-004 | 07 §8 | F2 |
| T-F3-11 | Conformidad OpenAPI, matriz de autorización REST completa, aislamiento de tenants | FR-API-001, FR-API-005, FR-API-006, NFR-SEC-004 | 18 §5, 06 §8 | T-F3-03..T-F3-08 |

**Criterios de salida:** todas las operaciones con `x-gs-phase` ≤ F3 implementadas y conformes; matriz de autorización 100 % verde; requisitos de F3 trazados.

---

## F4 — Stream en nodo único (3–4 semanas) → M1

| ID | Tarea y entregable | Requisitos | Leer | Depende |
|---|---|---|---|---|
| T-F4-01 | Tickets: `POST /v1/stream/tickets` y canje atómico en el upgrade | FR-AUTH-006 | 06 §5, 08 §3 | F3 |
| T-F4-02 | Conexión: upgrade, subprotocolos, hello, máquina de estados, heartbeat, inactividad, límites de trama y mensajes, códigos de cierre | FR-STR-001, FR-STR-005 | 08 §2–§6.5, §8 | T-F4-01 |
| T-F4-03 | `AuthzCache` con `mver`, invalidación por bus, revalidación por TTL, revalidación del principal | NFR-SEC-002 | 04 §6.2, ADR-0009 | T-F4-02 |
| T-F4-04 | Suscripción sin huecos, snapshot y `MemberStatus` con minimización | FR-STR-002, FR-PRS-001, FR-PRS-004 | 08 §6.3, 05 §8 | T-F4-03 |
| T-F4-05 | Publicación: precondiciones, validación, CAS de secuencia, `sequence_floor`, snapshot, rate limit, acks; vectores por stream | FR-LOC-001..FR-LOC-008, FR-STR-004 | 08 §6.4, §7, 09 §3–§4, ADR-0008 | T-F4-03 |
| T-F4-06 | Fan-out por bus (ruta única), coalescencia y backpressure | FR-STR-003, FR-STR-006 | 04 §8, 08 §10 | T-F4-05 |
| T-F4-07 | Eventos de control y revocación en vivo (`SubscriptionEnded`, cierres 4401/4403) | FR-STR-008, FR-STR-011, FR-AUTH-005 | 08 §9, 05 §10 | T-F4-04 |
| T-F4-08 | Presencia, gracia de desconexión y conexión única | FR-PRS-002, FR-STR-007 | 04 §6.3, 08 §6.7 | T-F4-04 |
| T-F4-09 | `AuthExpiring` / `AuthRefresh` | FR-AUTH-007 | 08 §6.6 | T-F4-02 |
| T-F4-10 | `GET /v1/sessions/{id}/snapshot` | FR-INT-002 | 07 §9.6 | T-F4-05 |
| T-F4-11 | `gs-testkit`: cliente de prueba y runner de escenarios (DSL completo); SCN-001..010 verdes con ambos subprotocolos (SCN-009 nocturno) | FR-PRV-001 | 18 §6, contracts/README §5 | T-F4-04..T-F4-09 |
| T-F4-12 | `gs-devclient`: conectar, suscribir y publicar trayectorias sintéticas | — | 04 §4 | T-F4-05 |
| T-F4-13 | Pruebas de privacidad: captura de logs sobre escenarios, inspección de PG y almacén efímero, borrado inmediato de snapshot | FR-PRV-004, NFR-PRIV-001, NFR-PRIV-002, NFR-SEC-003 | 10 §8, 18 §7 | T-F4-11 |

**Criterios de salida (M1):** SCN-001..010 verdes; plazo de revocación medido y dentro de NFR-SEC-002; demo con `gs-devclient` (dos publicadores y un lector); release `server-v0.1.0-alpha.1`.

---

## F5 — SDK Flutter núcleo, Android (4–6 semanas) → M2

Adelantable: T-F5-01..T-F5-05 pueden empezar cuando T-F4-02 esté cerrada.

| ID | Tarea y entregable | Requisitos | Leer | Depende |
|---|---|---|---|---|
| T-F5-01 | Estructura `sdks/flutter/geostream` + `example`; verificar nombres en pub.dev; fijar versiones mínimas tras revisar dependencias; matriz de compatibilidad | NFR-COMPAT-001 | 12 §2, §17 | F4 |
| T-F5-02 | Generación Dart de los `.proto` y check de sincronía en CI | — | contracts/README §2 | T-F5-01 |
| T-F5-03 | Modelos públicos, jerarquía de excepciones y mapeo exhaustivo de `ErrorCode` | FR-SDK-001, FR-SDK-009 | 12 §4–§6 | T-F5-02 |
| T-F5-04 | `RestClient` y `CredentialManager` | FR-SDK-002 | 12 §8, 06 §2 | T-F5-03 |
| T-F5-05 | `SequenceAllocator` y `KeyValueStore` | FR-SDK-005 | 12 §9 | T-F5-03 |
| T-F5-06 | `StreamConnection`: estados, reconexión, tabla de cierres, `AuthRefresh`, `ClockSync` | FR-SDK-004, FR-SDK-006 | 12 §8, §10, 08 §8 | T-F5-04 |
| T-F5-07 | `LocationSource` (geolocator), permisos, conversión de plataforma, `SampleValidator` con vectores | FR-SDK-003, FR-SDK-007 | 12 §7, §11 | T-F5-03 |
| T-F5-08 | `PublishPipeline`: perfiles, intervalo mínimo, fix de mantenimiento, `latestOnly`, detenciones pendientes | FR-SDK-007, FR-SDK-008 | 12 §11, §13 | T-F5-06, T-F5-07 |
| T-F5-09 | `SessionView` y `sessions.watch` | FR-SDK-012 | 12 §14 | T-F5-06 |
| T-F5-10 | Android en segundo plano (foreground service) | FR-SDK-010 | 12 §12.1 | T-F5-08 |
| T-F5-11 | App de ejemplo (backend de tokens mínimo, crear/unirse, compartir, lista de miembros) | FR-SDK-013 | 12 §4 | T-F5-09 |
| T-F5-12 | Pruebas con *fakes*, integración contra servidor real, checklist de dispositivos Android | FR-SDK-015, FR-SDK-016 | 12 §16, 18 §9 | T-F5-11 |
| T-F5-13 | **Congelar contrato v1**: OpenAPI `info.version: 1.0.0`, activar `buf breaking` y `oasdiff` obligatorios | NFR-COMPAT-002 | 08 §11, 07 §11 | T-F5-12 |

**Criterios de salida (M2):** dos teléfonos Android comparten en tiempo real con la app de ejemplo contra un servidor desplegado; checklist de dispositivos Android completo; contrato v1 congelado; `sdk-flutter-v0.1.0-alpha.1`.

---

## F6 — Multi-instancia, rendimiento y despliegue (3–4 semanas)

Adelantable: puede empezar al cerrar F4, en paralelo con F5 (crates distintos).

| ID | Tarea y entregable | Requisitos | Leer | Depende |
|---|---|---|---|---|
| T-F6-01 | `gs-ephemeral` y `gs-bus` sobre Redis/Valkey (scripts Lua, Pub/Sub con refcount); suite parametrizada verde | FR-STR-009, FR-PRS-003 | 11 §3, 04 §6, ADR-0010 | F4 |
| T-F6-02 | Desplazamiento de conexión y presencia entre instancias | FR-STR-007 | 04 §6.3 | T-F6-01 |
| T-F6-03 | Degradación y recuperación ante caída del almacén efímero | NFR-RES-002 | 04 §7.6 | T-F6-01 |
| T-F6-04 | Drenaje en SIGTERM | FR-STR-010, NFR-RES-003 | 08 §6.8 | T-F6-01 |
| T-F6-05 | Dockerfile, compose de referencia, Caddyfile | FR-OPS-005, NFR-COST-001 | 16 §10 | T-F6-01 |
| T-F6-06 | Escenarios multi-nodo en CI | FR-STR-009 | 18 §6 | T-F6-02 |
| T-F6-07 | `gs-loadgen` con perfiles P-S, P-M, P-L, P-FLEET, P-STORM | — | 18 §8 | T-F6-01 |
| T-F6-08 | Benchmarks y optimización hasta cumplir NFR de rendimiento y escala (o ADR con desviación medida y plan) | NFR-PERF-001, NFR-PERF-002, NFR-PERF-003, NFR-PERF-004, NFR-SCALE-001, NFR-SCALE-002 | 03 §3, 18 §8 | T-F6-07 |
| T-F6-09 | Pruebas de caos nocturnas | NFR-RES-002 | 18 §2 | T-F6-03 |
| T-F6-10 | Catálogo completo de métricas y alertas de ejemplo | NFR-OBS-001 | 16 §4–§5 | T-F6-01 |

**Criterios de salida:** benchmarks publicados en `docs/benchmarks/` con resultados medidos; escenarios multi-nodo y caos verdes.

---

## F7 — iOS y endurecimiento móvil (3–4 semanas) → M3

| ID | Tarea y entregable | Requisitos | Leer | Depende |
|---|---|---|---|---|
| T-F7-01 | iOS: permisos, actualizaciones en segundo plano, `Info.plist` del ejemplo | FR-SDK-011 | 12 §12.2 | F5 |
| T-F7-02 | Build iOS en CI (runner macOS) y app de ejemplo iOS | FR-SDK-013 | 18 §10 | T-F7-01 |
| T-F7-03 | Matriz completa de dispositivos y medición de batería | NFR-MOB-001, NFR-MOB-002 | 18 §9 | T-F7-02 |
| T-F7-04 | Endurecimiento: pausa/reanudación por permisos, auto-stop, desconexión en segundo plano, UX de `Superseded`, detenciones pendientes tras reinicio | FR-SDK-003, FR-SDK-008 | 12 §7, §8, §13 | T-F7-01 |
| T-F7-05 | Guía de plataforma para integradores (declaraciones de tiendas, batería de fabricantes) | FR-INT-004 (parcial) | 12 §12, 15 | T-F7-03 |

**Criterios de salida (M3):** Android e iOS probados en dispositivos reales con informe; servidor multi-nodo desplegado; versiones beta publicadas.

---

## F8 — Integraciones (3–4 semanas)

Adelantable: T-F8-01 puede empezar al cerrar F3.

| ID | Tarea y entregable | Requisitos | Leer | Depende |
|---|---|---|---|---|
| T-F8-01 | Webhooks: CRUD, outbox transaccional, worker de entrega, firma, SSRF, deshabilitación | FR-WHK-001, FR-WHK-002, FR-WHK-003, FR-WHK-004, FR-WHK-005 | 07 §10, 10 §5.3, ADR-0015 | F3 |
| T-F8-02 | Consumidor del lado servidor: tickets de aplicación, suscripción con `locations:read`, auditoría, vida máxima de conexión | FR-INT-001 | 15 §5, 06 §5 | F6 |
| T-F8-03 | Ingesta HTTP por lotes | FR-LOC-009 | 07 §9.8 | F4 |
| T-F8-04 | CORS por aplicación | FR-INT-003 | 07 §12 | F3 |
| T-F8-05 | Flutter Web (sin segundo plano) | FR-SDK-014 | 12 | F5 |
| T-F8-06 | `examples/`: backend de tokens, receptor de webhooks y consumidor de stream, ejecutados en CI; escenarios SCN-011 (webhooks) y SCN-012 (flota con consumidor) | FR-INT-004 | 15 | T-F8-01, T-F8-02 |
| T-F8-07 | Quickstart verificado en < 30 min | NFR-DX-001 | 15 §2 | T-F8-06 |

**Criterios de salida:** webhooks y consumidores verificados por escenarios; ejemplos en CI; quickstart validado.

---

## F9 — Mapas (2–3 semanas) → M4

| ID | Tarea y entregable | Requisitos | Leer | Depende |
|---|---|---|---|---|
| T-F9-01 | `geostream_maps`: interfaces y `ProviderStack` con reglas de compatibilidad | FR-MAP-001, FR-MAP-002 | 13 §3–§4 | F5 |
| T-F9-02 | Adaptador MapLibre con atribución | FR-MAP-003 | 13 §5.1 | T-F9-01 |
| T-F9-03 | Interpolación visual etiquetada | FR-MAP-005 | 13 §6 | T-F9-02 |
| T-F9-04 | Adaptador Google Maps | FR-MAP-004 | 13 §5.2 | T-F9-01 |
| T-F9-05 | Revalidar términos, cuotas, costos y atribución; actualizar `13` §4 con fecha | FR-MAP-006 | 13 §2 | T-F9-02 |
| T-F9-06 | App de ejemplo con mapa; prueba con adaptador que falla | NFR-RES-001 | 13 §1 | T-F9-02 |

**Criterios de salida (M4):** un integrador puede construir un producto completo (sesiones, compartición, mapa, webhooks, backend consumidor) siguiendo la documentación; release candidate.

---

## F10 — Privacidad completa y seguridad (2–3 semanas)

| ID | Tarea y entregable | Requisitos | Leer | Depende |
|---|---|---|---|---|
| T-F10-01 | Borrado y exportación de subject | FR-PRV-002 | 10 §7, 07 §9.5 | F8 |
| T-F10-02 | Jobs de retención | FR-PRV-003 | 10 §7.1, 11 §5 | F8 |
| T-F10-03 | Fuzzing en CI nocturno | NFR-SEC-005 | 18 §7 | F4 |
| T-F10-04 | Revisión del modelo de amenazas, checklist de liberación completa, prueba de penetración interna (externa si hay presupuesto) | NFR-SEC-004 | 10 §4, §11 | T-F10-01 |
| T-F10-05 | Rotación de `GS_DATA_ENCRYPTION_KEY` con re-cifrado | — | 10 §5.6, 16 §8 | F8 |
| T-F10-06 | Pruebas ampliadas de SSRF y de todos los límites de abuso | FR-WHK-004 | 10 §5.3–§5.4 | F8 |

**Criterios de salida:** checklist de seguridad verde; fuzzing 7 noches sin fallos; borrado verificado en todas las tablas.

---

## F11 — Geocercas (2–3 semanas)

Adelantable: puede empezar al cerrar F6.

| ID | Tarea y entregable | Requisitos | Leer | Depende |
|---|---|---|---|---|
| T-F11-01 | Migración `0003_geofences.sql`, CRUD y validación de geometría | FR-GEO-001 | 14 §2–§3, 11 §2 | F6 |
| T-F11-02 | Motor de evaluación con histéresis y estado efímero | FR-GEO-002 | 14 §4 | T-F11-01 |
| T-F11-03 | Índice R-tree por tenant con invalidación por bus | FR-GEO-004 | 14 §6 | T-F11-02 |
| T-F11-04 | `GeofenceEvent` por stream y webhooks | FR-GEO-003 | 14 §5 | T-F11-02, T-F8-01 |
| T-F11-05 | Trayectorias sintéticas con ruido; escenario SCN-013 | FR-GEO-002 | 14 §7 | T-F11-04 |
| T-F11-06 | Benchmark con `max_geofences_per_tenant` | NFR-PERF-001 | 18 §8 | T-F11-03 |

**Criterios de salida:** transiciones exactas en trazas ruidosas; NFR-PERF-001 mantenido con el máximo de geocercas.

---

## F12 — Preparación de producción (3–4 semanas) → M5 v1.0 GA

| ID | Tarea y entregable | Requisitos | Leer | Depende |
|---|---|---|---|---|
| T-F12-01 | Runbooks completos y ensayo de rotaciones (JWT, API keys, cifrado) | FR-OPS-006 | 16 §8 | F10 |
| T-F12-02 | Backups y ejercicio de restauración documentado | FR-OPS-006 | 16 §9 | F10 |
| T-F12-03 | Tablero de SLO y alertas; ≥ 30 días de medición en beta | NFR-AVAIL-001 | 16 §5 | F7 |
| T-F12-04 | Benchmarks finales P-M y P-L con la versión candidata | NFR-PERF-001..004 | 18 §8 | F11 |
| T-F12-05 | Documentación pública (sitio a partir de `docs/` y OpenAPI renderizado), guía de migración, matriz de compatibilidad final | NFR-DOC-001, NFR-COMPAT-001 | 00, 15 | F9 |
| T-F12-06 | Auditoría de trazabilidad: todo requisito MUST ≤ F12 con pruebas verdes | — | TRACEABILITY | F11 |
| T-F12-07 | Publicación: `server-v1.0.0`, contratos 1.0.0, `sdk-flutter-v1.0.0`, imagen de contenedor con SBOM, changelog | NFR-MAINT-001 | 19 §8 | T-F12-06 |

**Criterios de salida (M5):** los de `01` §9.

---

---

## Pista E — Motor de ubicación y movilidad (E0–E9)

**Estado:** E0 en curso. Documentos: `22` (trayectoria), `23` (movilidad),
`24` (historial y analítica), `25` (dispositivos edge), `26` (núcleo y bindings).
Decisiones: ADR-0021..0032. Origen archivado:
`docs/proposals/GeoStream_Addendum_A02_Motor_Ubicacion_Movilidad.md`.

### Por qué esta pista está separada

La línea base F0–F12 construye **compartición en tiempo real**. La pista E construye
**el motor** alrededor: decide cuándo publicar, estima la trayectoria, ofrece
pools y ETA, guarda historial con consentimiento, aprende del mapa y llega a
dispositivos sin internet.

Está separada del roadmap principal por una razón concreta, no estética:

> **La línea base no se toca.** Cada capacidad nueva es aditiva, está detrás de un
> interruptor de política del tenant apagado por defecto, y un cliente escrito
> antes de la pista E sigue funcionando sin cambios (`NFR-COMPAT-003`, SCN-026).

Si la pista E se mezclara con F5–F12, cada PR de la línea base tendría que
reevaluar si algo de lo que hace sigue siendo válido, y la promesa de «v1.0 es un
producto que se puede integrar y ya está» dejaría de ser cierta.

### Mapa de fases de la pista E

```mermaid
flowchart LR
  E0["E0 Documentación,<br/>contratos y ADR"] --> E1["E1 Núcleo Rust<br/>compartido"]
  E1 --> E2["E2 Motor de<br/>trayectoria"]
  E1 --> E3["E3 Núcleo de<br/>dispositivo"]
  E2 --> E4["E4 Pools"]
  E3 --> E4
  E3 --> E8["E8 Dispositivos<br/>edge"]
  E4 --> E5["E5 Herramientas<br/>y BYOK"]
  E2 --> E6["E6 Historial<br/>y analítica"]
  E5 --> E7["E7 Capa de mapa<br/>aprendida"]
  E6 --> E7
  E7 --> E9["E9 Bindings<br/>multilingüe"]
  E8 --> E9
```

| Fase | Entregable | Requisitos | Depende |
|---|---|---|---|
| **E0** | Documentación `22`–`26`, ADR-0021..0032, contrato ampliado, vectores y escenarios | — (documental) | — |
| **E1** | `gs-geo`, `gs-client-core`, `gs-client-runtime`, `gs-ffi` esqueleto, compilación multiarquitectura | FR-CORE-001..003, FR-OPS-007 | E0 |
| **E2** | `gs-track` en el servidor: Kalman, plausibilidad, odómetro, paradas | FR-TRK-001..007, NFR-QUAL-001..003 | E1 |
| **E3** | Muestreo adaptativo, publicación por cambio, cola cifrada, predicción local, SDK Flutter sobre el núcleo | FR-SMP-001..006, FR-CORE-001..003, FR-BND-001..003 | E1 |
| **E4** | `SessionMode.POOL`, índice espacial vivo, `nearby`/`rings`/`density` | FR-POOL-001..006 | E2, E3 |
| **E5** | ETA, asignación, Provider Gateway BYOK, geocercas extendidas, grafo vial propio | FR-MOB-001..003, FR-GEO-005..007, FR-LRN-004 | E4 |
| **E6** | Historial opt-in, triple llave, k-anonimato, viajes, exportaciones | FR-HIS-001..003, FR-HIS-005..007, FR-ANA-001..004 | E2 |
| **E7** | Capa de mapa aprendida, lugares DBSCAN, map-matching, perfil de velocidad | FR-LRN-001..003, FR-HIS-004 | E5, E6 |
| **E8** | Tags BLE, red GeoStream Beacon, DULT, LPWAN, relevo en el cliente | FR-EDGE-001..008 | E3 |
| **E9** | Bindings TypeScript, Go y Ruby + conformidad multilingüe | FR-BND-004..007, NFR-COMPAT-003 | E7, E8 |

Estimación E1–E9: 26–38 semanas con dos personas y agentes de código. **No son
compromisos** y las fases E5, E6 y E8 son truncables: el motor sigue siendo útil
sin ellas.

### E0 — Documentación, contratos y ADR (en curso)

Sin código de producción. Cierra el marco que las demás fases implementan.

| ID | Tarea y entregable | Requisitos | Leer | Depende |
|---|---|---|---|---|
| T-E0-01 | `docs/01` v0.3.0: dos mitades, G-11..G-15, NG-4/5/6, UC-09..13, P-E1..10, criterios de GA | — | 01, ADR-0021 | — |
| T-E0-02 | `docs/03` + `TRACEABILITY`: ~70 requisitos nuevos, `FR-MAP-005` enmendado, `FR-NAT-001` retirado | todos los `*-E` | 03, ADR-0021..0032 | T-E0-01 |
| T-E0-03 | ADR-0021..0032; `ADR-0005/0012/0014/0016` sustituidos; `0013` ampliado | — | 19, 00 | T-E0-01 |
| T-E0-04 | `docs/22`..`26` con el detalle técnico normativo de la pista | — | 00 §1 | T-E0-03 |
| T-E0-05 | Enmiendas a `docs/02` y `docs/05`–`docs/20` | — | 22–26 | T-E0-04 |
| T-E0-06 | `contracts/`: protos, OpenAPI, vectores `TRK/SMP/POOL/ETA/PRD/ASG/EDGE`, SCN-014..026 | NFR-COMPAT-003 | contracts/README, 20 §4 | T-E0-05 |
| T-E0-07 | Este roadmap: pista E, criterios de GA y re-alcance de F5/F13/F15 | — | 01 §9, 22–26 | T-E0-06 |
| T-E0-08 | `tools/`: fases `E<n>`, scopes nuevos en la matriz de auth, reglas de crates, prefijos `tag_`/`trp_`/`plc_`/`exp_`/`lpw_` | — | 18 §10.1 | T-E0-06 |

**Criterios de salida (E0):** `geostream-tools all` verde con las ocho
comprobaciones; `buf lint` y `buf breaking` contra `main` sin errores;
`oasdiff` sin cambios incompatibles; los ocho documentos normativos
aproximadamente completos; ninguna tarea de la pista E empieza sin haber leído
`22`–`26`.

**Lo que E0 NO hace, a propósito:** no escribe una línea de `gs-track`,
`gs-mobility` ni `gs-history`. Un contrato sin implementación es reversible; una
implementación sobre un contrato que cambia es trabajo rehecho.

### E1 — Núcleo Rust compartido (4–5 semanas)

| ID | Tarea y entregable | Requisitos | Leer | Depende |
|---|---|---|---|---|
| T-E1-01 | `gs-geo`: haversine, geodesia, Douglas-Peucker, rumbo; radio terrestre declarado | — | 26 §1, 17 §2.11 | E0 |
| T-E1-02 | `gs-client-core`: traits de puerto, máquina de estados, suavizado de rumbo, muestreo; sin I/O ni tokio | FR-CORE-001 | 26 §1 | T-E1-01 |
| T-E1-03 | `gs-client-runtime`: implementación de puertos con `tokio` + plataforma | FR-CORE-002 | 26 §1 | T-E1-02 |
| T-E1-04 | `gs-ffi`: superficie `extern "C"` mínima, handles opacos, `catch_unwind`, `// SAFETY:` en cada bloque `unsafe` | FR-CORE-003 | 26 §3.2, 19 §6.2.1 | T-E1-03 |
| T-E1-05 | Compilación multiarquitectura Linux en CI y matriz de objetivos | FR-OPS-007 | 26 §2, 26 §5 | T-E1-03 |
| T-E1-06 | Propiedades de `gs-client-core`: idempotencia de publicación, límite de intervalo, conservación de la última muestra | NFR-QUAL-001 | 18 §3 | T-E1-02 |

**Criterios de salida:** `#![forbid(unsafe_code)]` se mantiene en todo el
workspace salvo `gs-ffi`; la suite de conformidad verde; `cargo deny` con las
dependencias nuevas (ADR por cada una).

### E2 — Motor de trayectoria en el servidor (3–4 semanas)

| ID | Tarea y entregable | Requisitos | Leer | Depende |
|---|---|---|---|---|
| T-E2-01 | Migración `0003_engine_state.sql` (`sessions.history_mode`, `memberships.consent_purposes`) | FR-TRK-006 | 11 §2.0 | E1 |
| T-E2-02 | Estado del filtro en el almacén efímero con control de versión y checkpoint | FR-TRK-006 | 22 §2.1, 11 §3.1 | T-E2-01 |
| T-E2-03 | Puerta de plausibilidad y `PlausibilityFlag`; vectores `TRK-PLAUS-*` | FR-TRK-002 | 22 §2.2 | T-E2-02 |
| T-E2-04 | Filtro de Kalman de velocidad constante y ZUPT; vectores `TRK-FILT-*` | FR-TRK-001, FR-TRK-003, NFR-QUAL-001 | 22 §2.3–§2.4 | T-E2-03 |
| T-E2-05 | Odómetro con puente de huecos; vectores `TRK-ODO-*` | FR-TRK-004, NFR-QUAL-002 | 22 §2.5 | T-E2-04 |
| T-E2-06 | Paradas online y `TrackEvent`; vectores `TRK-STOP-*` | FR-TRK-005, NFR-QUAL-003 | 22 §2.6 | T-E2-05 |
| T-E2-07 | Paso de estimación en el algoritmo de aceptación (4-bis de `09` §4); SCN-014, SCN-015 | FR-TRK-001, FR-LOC-006 | 09 §4, 08 §6.4 | T-E2-06 |
| T-E2-08 | Presupuesto por muestra medido (P-TRK) y modo degradado | NFR-PERF-005, NFR-PERF-006 | 18 §8.3, 22 §2.8 | T-E2-07 |
| T-E2-09 | Webhooks `track.stop_*` sin coordenadas | FR-TRK-007, FR-WHK-005 | 07 §10.2 | T-E2-06 |

**Criterios de salida:** SCN-014 y SCN-015 verdes en ambos subprotocolos;
presupuesto por muestra **medido** y publicado en `docs/benchmarks/`; la
aceptación no depende en ningún caso de la disponibilidad del motor.

### E3 — Núcleo de dispositivo: muestreo, cola y predicción (3–4 semanas)

| ID | Tarea y entregable | Requisitos | Leer | Depende |
|---|---|---|---|---|
| T-E3-01 | Muestreo adaptativo por zonas; vectores `SMP-INT-*` | FR-SMP-001 | 22 §3.2 | E1 |
| T-E3-02 | Publicación por cambio significativo; vectores `SMP-PUB-*` | FR-SMP-002 | 22 §3.3 | T-E3-01 |
| T-E3-03 | `SamplingPolicy` en `ServerHello` + acotamiento en cliente; vectores `SMP-CLAMP-*` | FR-SMP-003 | 22 §3.4, 08 §6.1 | T-E3-02 |
| T-E3-04 | Cola sin conexión cifrada y entrega por lotes; vectores `SMP-OFF-*` | FR-SMP-006 | 22 §3.5, 12 §13.2 | T-E3-02 |
| T-E3-05 | Predicción visual local; vectores `SMP-PRED-*`; rechazo de `PREDICTED`/`INTERPOLATED` publicados | FR-SMP-005 | 22 §3.6, 08 §7.6 | T-E3-02 |
| T-E3-06 | Despertar por región del sistema operativo y geocercas asistidas en el dispositivo | FR-SMP-004, FR-GEO-007 | 22 §3.7 | T-E3-01 |
| T-E3-07 | Ahorro del muestreo adaptativo y tiempo de reacción, **medidos** en dispositivo real | NFR-MOB-002, NFR-MOB-003, NFR-MOB-005 | 18 §9 | T-E3-02 |
| T-E3-08 | SDK Dart: `geostream_core` (FFI) con implementación Dart equivalente, más bindings Kotlin y Swift | FR-BND-001, FR-BND-002, FR-BND-003 | 26 §3.1, 12 §3 | T-E3-04, T-E1-04 |

**Criterios de salida:** SCN-026 verde; el cliente antiguo sin cambios; batería
**medida** en dispositivo real, con informe.

### E4 — Pools de disponibilidad (3–4 semanas)

| ID | Tarea y entregable | Requisitos | Leer | Depende |
|---|---|---|---|---|
| T-E4-01 | `SessionMode.POOL`, `INV-POOL-1`, validación de capacidades | FR-POOL-001 | 05 §4.2.1 | E2 |
| T-E4-02 | Índice espacial vivo con H3 y R-tree; borrado síncrono | FR-POOL-003 | 23 §2, ADR-0026 | T-E4-01 |
| T-E4-03 | `nearby` y `rings` con prefiltrado de ETA | FR-POOL-004, FR-POOL-005 | 23 §3.1–§3.2 | T-E4-02 |
| T-E4-04 | `density` con desplazamiento, cuantización de rumbo y retraso | FR-POOL-006 | 23 §3.3 | T-E4-02 |
| T-E4-05 | `availability` y `attributes` con minimización en `MemberStatus` | FR-POOL-002 | 23 §1.1, 05 §8 | T-E4-01 |
| T-E4-06 | SCN-016, SCN-017, SCN-018 verdes | INV-POOL-1, INV-PRS-2 | contracts/README §5.1.1 | T-E4-05 |
| T-E4-07 | P-POOL medido con 50 000 conductores en línea | NFR-PERF-007 | 18 §8.3 | T-E4-06 |

**Criterios de salida:** los tres escenarios verdes en los cuatro modos del
runner; p99 de `nearby` **medido**; documentado que el índice exige afinidad de
sesión (`16` §7).

### E5 — Herramientas de movilidad y BYOK (3–4 semanas)

| ID | Tarea y entregable | Requisitos | Leer | Depende |
|---|---|---|---|---|
| T-E5-01 | Estimador de ETA propio (distancia, modo, circunvalidad, matriz aprendida) | FR-MOB-001 | 23 §4 | E4 |
| T-E5-02 | Asignación: Hungarian y min-cost-flow, con capacidad | FR-MOB-002, NFR-PERF-008 | 23 §5 | T-E5-01 |
| T-E5-03 | Provider Gateway: configuración BYOK cifrada en reposo, manifiesto de términos, caché acotada | FR-MOB-003 | 13 §8, ADR-0028 | T-E5-01 |
| T-E5-04 | Minimización estricta de la petición al proveedor (T-29) y prueba de que no filtra identificadores | FR-MOB-003, FR-PRV-004 | 13 §8.1, 10 T-29 | T-E5-03 |
| T-E5-05 | `PROVIDER_UNAVAILABLE` sin degradación silenciosa; SCN-022, SCN-023 | FR-MOB-001 | 09 §6, 07 §9.10 | T-E5-03 |
| T-E5-06 | Geocercas extendidas: `DWELL` y proximidad dinámica; prohibición en `POOL` | FR-GEO-005..007 | 14 §5.1 | T-E4-01 |
| T-E5-07 | Grafo vial propio: herramienta `geostream graph build`, carga, R-tree, jerarquías de contracción | FR-LRN-004 | 23 §7, ADR-0032 | T-E5-01 |

**Criterios de salida:** SCN-022 y SCN-023 verdes; los tres casos `*_DEG-*` de
`mobility_tools.json` verdes; revisión legal de las obligaciones de ODbL cerrada
antes de distribuir bases derivadas.

### E6 — Historial opt-in y analítica k-anónima (4–5 semanas)

| ID | Tarea y entregable | Requisitos | Leer | Depende |
|---|---|---|---|---|
| T-E6-01 | **Revisión de DPIA y textos de consentimiento por propósito** | FR-HIS-001 | 10 §9 | E0 |
| T-E6-02 | Migración `0004_history.sql` con `location_points` particionada | FR-HIS-001 | 11 §2.0 | T-E6-01 |
| T-E6-03 | Escritor de historial con triple llave, canal acotado y lotes | FR-HIS-001, NFR-PERF-009 | 24 §2 | T-E6-02 |
| T-E6-04 | `INV-SHR-5`: `consent_purposes` solo del propio subject; SCN-020 | FR-HIS-001, FR-ANA-002 | 05 §7.2, 24 §1 | T-E6-03 |
| T-E6-05 | Retención por eliminación de particiones y supresión del subject | FR-HIS-005 | 24 §5 | T-E6-03 |
| T-E6-06 | Viajes, paradas y polilínea `gsp1` simplificada, y consulta de trazas | FR-HIS-003 | 24 §3–§4 | T-E6-03 |
| T-E6-07 | Trabajo de agregación con supresión k-anónima en el trabajo, no en la consulta | FR-ANA-001, NFR-PRIV-003 | 24 §6 | T-E6-05 |
| T-E6-08 | Mapas de calor, matriz origen-destino, perfil de movilidad y `replay_trace` | FR-ANA-002, FR-ANA-004, FR-HIS-007 | 24 §4 | T-E6-07 |
| T-E6-09 | Exportaciones NDJSON/CSV con URL de TTL corto | FR-HIS-006 | 24 §4 | T-E6-06 |
| T-E6-10 | SCN-019, SCN-021 verdes; P-HIST medido | NFR-PERF-009, NFR-PRIV-003 | 18 §8.3 | T-E6-08 |
| T-E6-11 | **Revisión legal antes del release de E6** (T-E6-01 no se cierra sin ella) | NFR-PRIV-003 | 10 §9 | T-E6-10 |

**Criterios de salida:** **T-E6-11 cerrada** antes de cualquier release de E6;
`history_allowed` y `analytics_allowed` siguen en `false` hasta entonces; SCN-019
y SCN-021 verdes.

> La fase E6 **no se puede publicar** sin la revisión legal. El riesgo no es
> técnico: es que el historial con consentimiento granular y el perfil de
> movilidad cambian el marco aplicable, y eso no lo decide un agente.

### E7 — Capa de mapa aprendida (3–4 semanas)

| ID | Tarea y entregable | Requisitos | Leer | Depende |
|---|---|---|---|---|
| T-E7-01 | Perfiles de velocidad por celda H3 y franja, con supresión | FR-LRN-001 | 24 §3.1 | E6 |
| T-E7-02 | Lugares aprendidos con DBSCAN (`eps_m`, `min_points`), derivados y anónimos | FR-LRN-002 | 24 §3.1 | T-E7-01 |
| T-E7-03 | Lugares aprendidos y perfiles de velocidad: API de consulta y pruebas de k-anonimato | FR-LRN-001, FR-LRN-002, NFR-PRIV-003 | 07 §9.11 | T-E7-01, T-E7-02 |
| T-E7-04 | Eventos de conducción estimados (aceleración y frenada) | FR-ANA-003 | 24 §3.2 | T-E7-01 |
| T-E7-05 | Map-matching con retardo fijo sobre grafo; calibración **medida** de σ y β | FR-LRN-003, FR-HIS-004 | 24 §3.3 | T-E7-01, T-E5-07 |
| T-E7-06 | Medición de la calibración (sin valores aflotados sin dato) | FR-HIS-004 | 20 §9.1 | T-E7-05 |
| T-E7-07 | Routear sobre el grafo vial propio (`POST /v1/tools/routes` con `OWN_GRAPH`) | FR-LRN-004 | 07 §9.10 | T-E7-05 |

**Criterios de salida:** toda constante de map-matching **medida** en una
región real y registrada con su entorno en `docs/benchmarks/`; sin celdas por
debajo de *k* en ninguna respuesta.

### E8 — Dispositivos edge: tags BLE y LPWAN (4–5 semanas)

| ID | Tarea y entregable | Requisitos | Leer | Depende |
|---|---|---|---|---|
| T-E8-01 | Migración `0005_edge.sql` (`tags`, `edge_reports`, `edge_lpwan_integrations`) sin columna `EIK` | FR-EDGE-001 | 11 §2.0 | E3 |
| T-E8-02 | Aprovisionamiento y registro de tags; la `EIK` no sale del dispositivo | FR-EDGE-001 | 25 §2.2 | T-E8-01 |
| T-E8-03 | Identificador efímero y rotación; estado `SEPARATED` | FR-EDGE-001 | 25 §2.3 | T-E8-02 |
| T-E8-04 | Trama BLE, reporte del buscador cifrado de extremo a extremo y **relevo en el núcleo del cliente** | FR-EDGE-002, FR-EDGE-003 | 25 §2.4–§2.5 | T-E8-03 |
| T-E8-05 | Almacenamiento opaco y consulta del dueño; SCN-024; no vinculable al buscador | FR-EDGE-002, NFR-PRIV-004 | 25 §2.6–§2.7 | T-E8-04 |
| T-E8-06 | **Protección DULT completa antes de cualquier release de E8** | FR-EDGE-004 | 25 §3 | T-E8-04 |
| T-E8-07 | Puente opcional de `25` §2.8, explícito y auditado | FR-EDGE-007 | 25 §2.8 | T-E8-05 |
| T-E8-08 | Ingesta LPWAN con firma HMAC y decodificador por modelo | FR-EDGE-005 | 25 §4 | T-E8-01 |
| T-E8-09 | Firmware de referencia del tag (COULD) y P-EDGE medido | FR-EDGE-006, FR-EDGE-008 | 25 §5, 18 §8.3 | T-E8-06 |

**Criterios de salida:** SCN-025 verde; **T-E8-06 cerrada** antes de cualquier
release de E8; el servidor nunca almacena la `EIK`, verificado por inspección del
esquema y no por confianza.

> E8 es la fase con más superficie regulatoria y de seguridad. Las protecciones
> DULT son **obligatorias**, no un extra: sin ellas el producto no se libera.

### E9 — Bindings multilingüe (4–6 semanas)

| ID | Tarea y entregable | Requisitos | Leer | Depende |
|---|---|---|---|---|
| T-E9-01 | Empaquetado y publicación de los objetivos ya implementados en E3 (Kotlin, Swift, Dart) | FR-BND-001, FR-BND-002, FR-BND-003 | 26 §2–§3.2 | E7, E8 |
| T-E9-02 | TypeScript (WASM), Go (cgo) y Ruby (FFI) | FR-BND-004, FR-BND-005, FR-BND-006 | 26 §2 | T-E9-01 |
| T-E9-03 | Conformidad cruzada en los seis lenguajes con tolerancias declaradas | FR-BND-007, NFR-COMPAT-003 | 26 §4, DR-20261012-03 | T-E9-02 |
| T-E9-04 | Fuzzing de la frontera FFI | NFR-SEC-005 | 18 §10.4 | T-E9-01 |
| T-E9-05 | Documentación de onboarding por lenguaje y ejemplos ejecutables | NFR-DX-001 | 15 §6.1 | T-E9-03 |

**Criterios de salida:** los seis lenguajes producen la misma salida en los
vectores `SMP-*` dentro de la tolerancia declarada; ningún `panic` cruza la
frontera; fuzzing sin hallazgos.

### Criterios de salida de GA por versión

| Versión | Alcance | Requisito de entrada |
|---|---|---|
| **v1.0 (M5)** | Línea base F0–F12, sin nada de la pista E | — |
| **v1.1** | E1, E2, E3: trayectoria, muestreo adaptativo, cola cifrada | E0 cerrada; P-TRK y batería **medidos** |
| **v1.2** | E4, E5, E7: pools, herramientas, capa de mapa | E1–E3 cerradas; revisión de ODbL |
| **v1.3** | E6: historial y analítica | **T-E6-11 cerrada**; DPIA y textos aprobados |
| **v1.4** | E8: dispositivos edge | **T-E8-06 cerrada**; revisión de seguridad |
| **v1.5** | E9: bindings multilingües | E7 y E8 cerradas |

Las versiones de la pista E **no invalidan** el número de versión del contrato:
todo es aditivo bajo `/v1` y `gs.v1`. El minor del protocolo sube solo si alguna
vez se necesita un campo que no se puede añadir de forma aditiva, y eso requiere
un ADR (`08` §11).

### Qué cambia en el resto del roadmap

| Fase | Cambio | Motivo |
|---|---|---|
| **F5** | El SDK Flutter pasa a ser **un** binding más. La lógica de cliente pasa a `gs-client-core`. `T-F5-08` (`PublishPipeline`) se reduce a la integración de plataforma. | ADR-0022 sustituye a ADR-0012 |
| **F13** | **Se elimina como fase.** Era «historial opt-in, cola offline acumulada cifrada, consulta de historial, calidad de trayectoria y map-matching». Todo eso es E3, E6 y E7, con contratos y ADR propios. | Evitar dos documentos normativos para lo mismo |
| **F15** | Los SDKs nativos dejan de ser una pista: son E9, porque comparten el núcleo Rust. Se conservan aquí solo `FR-AUTH-009`, `FR-ADM-004` y `FR-INV-004`. | ADR-0022, ADR-0031 |
| **F14 (MQTT)** | Sigue en pie, pero depende de E8: el mismo modelo de tags y reportes hace el MQTT mucho más barato. | ADR-0029 |
| **F16 (E2EE)** | Sigue en pie y ahora es **parcialmente falso**: los tags Beacon ya tienen E2EE. La sesión y REST siguen sin ella. | `10` T-22 |

Cada pista requiere, antes de empezar: un caso de uso real de al menos un integrador, un ADR aprobado y, cuando aplique, una revisión legal o de seguridad.

| Fase | Pista | Requisitos | Criterio de entrada específico |
|---|---|---|---|
| F13 | **Eliminada.** Sus cuatro elementos son ahora E3 (cola cifrada), E6 (historial), E2 (calidad de trayectoria) y E7 (map-matching), cada uno con su propio ADR y su propia fase. | — | — |
| F14 | Puente MQTT para IoT | FR-IOT-001 | ADR de broker y autenticación de dispositivos |
| F15 | **Parcialmente eliminada.** Los SDKs nativos son ahora E9 y las geocercas en dispositivo son E3 (`FR-GEO-007`): comparten el núcleo Rust y no tienen sentido como pista aparte. Quedan aquí la API de administración, los tokens firmados por el integrador y la aprobación de solicitudes de unión. | FR-AUTH-009, FR-ADM-004, FR-INV-004 | Demanda medida; ADR por cada elemento |
| F16 | Modo E2EE y transporte P2P WebRTC | FR-E2E-001, FR-P2P-001 | Revisión criptográfica externa; aceptación explícita de perder snapshots en claro y geocercas del servidor. Ojo: los tags Beacon de E8 **ya** tienen E2EE (`10` T-22), así que esta pista cubre sesión y REST, no el producto entero |
