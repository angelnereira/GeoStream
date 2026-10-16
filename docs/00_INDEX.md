# 00 — Índice, gobierno y convenciones

**ID:** GS-DOC-000 · **Versión del paquete documental:** 0.3.0 · **Fecha:** 2026-10-15
**Estado:** baseline normativo v0.2.0 más la pista de fases E (motor de ubicación y movilidad), adoptada según `docs/proposals/GeoStream_Addendum_A02_Motor_Ubicacion_Movilidad.md`.

## 1. Principio de gobierno

> GeoStream es un proyecto **gobernado por la documentación**. Los documentos de `docs/` y los contratos de `contracts/` definen requisitos, reglas, parámetros, validaciones y criterios de aceptación. El código existe para cumplirlos.
>
> Si el código y la documentación divergen, el cambio **no está terminado**. Si dos documentos se contradicen, **nadie adivina**: se detiene el trabajo afectado y se abre una solicitud de decisión (`docs/templates/DECISION_REQUEST.md`).

Corolarios obligatorios:

1. Ningún comportamiento observable (API, protocolo, validación, permisos, retención, configuración, métricas) se implementa sin estar especificado.
2. Todo cambio de comportamiento modifica primero, o en el mismo PR, el documento y el contrato correspondientes, y sus pruebas.
3. Una regla vive en **un solo lugar**. Los demás documentos la referencian por ID o por sección; no la copian.
4. Los valores numéricos (límites, timeouts, TTL) viven en `17_CONFIGURATION.md`. Los demás documentos citan el nombre del parámetro.

## 2. Mapa de documentos

| Archivo | ID | Gobierna | Léelo cuando… |
|---|---|---|---|
| `00_INDEX.md` | GS-DOC-000 | Gobierno, precedencia, convenciones | Siempre, primero |
| `01_VISION_SCOPE.md` | GS-VIS-001 | Visión, alcance, no objetivos, actores, casos de uso | Necesites contexto de producto |
| `02_GLOSSARY.md` | GS-GLO-001 | Vocabulario canónico | Un término sea ambiguo |
| `03_REQUIREMENTS.md` | GS-REQ-001 | Requisitos funcionales (FR) y no funcionales (NFR) con fase y criterio de aceptación | Antes de cualquier tarea |
| `04_ARCHITECTURE.md` | GS-ARC-001 | Arquitectura del sistema, componentes, crates, flujos, despliegue | Toques estructura o flujos |
| `05_DOMAIN_MODEL.md` | GS-DOM-001 | Entidades, invariantes, máquinas de estado, capacidades | Toques reglas de negocio |
| `06_IDENTITY_AUTH.md` | GS-IAM-001 | Principales, API keys, tokens, tickets, scopes, matriz de autorización, revocación | Toques autenticación o permisos |
| `07_REST_API.md` | GS-API-001 | Semántica del contrato REST y webhooks | Toques endpoints HTTP |
| `08_STREAM_PROTOCOL.md` | GS-PRO-001 | Protocolo WebSocket: handshake, mensajes, estados, cierre, backpressure | Toques el stream |
| `09_VALIDATION_ERRORS.md` | GS-VAL-001 | Reglas de validación por campo, algoritmo de aceptación, catálogo de errores | Valides datos o devuelvas errores |
| `10_SECURITY_PRIVACY.md` | GS-SEC-001 | Modelo de amenazas, controles, privacidad, retención, registro seguro | Siempre que toques datos, red, auth o logs |
| `11_DATA_STORAGE.md` | GS-DAT-001 | Esquema PostgreSQL, keyspace Redis/Valkey, migraciones, jobs | Toques persistencia |
| `12_FLUTTER_SDK.md` | GS-SDK-001 | SDK Flutter: API pública, estados, errores, capa nativa, background | Trabajes en `sdks/flutter` |
| `13_MAPS_PROVIDERS.md` | GS-MAP-001 | Adaptadores de mapas, geocoding, rutas; licencias | Trabajes en adaptadores de mapas |
| `14_GEOFENCING.md` | GS-GEO-001 | Geocercas: modelo, evaluación, eventos | Trabajes en F11 |
| `15_INTEGRATION_GUIDE.md` | GS-INT-001 | Patrones de integración para sistemas terceros | Diseñes ejemplos o flujos de integración |
| `16_OPERATIONS.md` | GS-OPS-001 | Despliegue, observabilidad, escalado, resiliencia, runbooks | Toques despliegue o métricas |
| `17_CONFIGURATION.md` | GS-CFG-001 | Todos los parámetros, defaults, rangos y políticas por tenant | Uses cualquier número configurable |
| `18_TESTING_STRATEGY.md` | GS-TST-001 | Tipos de prueba, vectores, escenarios, CI | Escribas pruebas o CI |
| `19_ENGINEERING_STANDARDS.md` | GS-ENG-001 | Reglas de código Rust/Dart, dependencias, git, versionado, DoD | Escribas código |
| `20_AGENT_PROTOCOL.md` | GS-AGT-001 | Ciclo de trabajo obligatorio para agentes de IA | Seas un agente |
| `21_ROADMAP.md` | GS-RMP-001 | Fases F0–F16 y E0–E9, tareas, entregables y criterios de salida | Elijas la siguiente tarea |
| `22_TRACKING_ENGINE.md` | GS-TRK-001 | Motor de trayectoria en el servidor, muestreo adaptativo y predicción visual en el cliente | Trabajes en `gs-track` o `gs-client-core` |
| `23_MOBILITY.md` | GS-MBL-001 | Pools de disponibilidad, índice espacial vivo, ETA, asignación y grafo vial propio | Trabajes en `gs-mobility` |
| `24_HISTORY_ANALYTICS.md` | GS-HAN-001 | Historial opt-in, capa de mapa aprendida, analítica y trabajos por lotes | Trabajes en `gs-analytics` o en el historial |
| `25_EDGE_DEVICES.md` | GS-EDG-001 | Red de búsqueda BLE cifrada, ingesta LPWAN y rastreadores celulares | Trabajes en `gs-edge` |
| `26_CORE_AND_BINDINGS.md` | GS-CBN-001 | Núcleo Rust sans-IO compartido, FFI y SDKs por lenguaje | Trabajes en `gs-client-core`, `gs-ffi` o `sdks/*` |
| `TRACEABILITY.md` | GS-TRC-001 | Requisito → pruebas → estado | Cierres una tarea |
| `PROGRESS.md` | GS-PRG-001 | Bitácora de avance (la actualizan los agentes) | Cierres una tarea |
| `adr/` | ADR-NNNN | Decisiones de arquitectura | Una decisión técnica esté en juego |
| `../contracts/` | — | Contratos máquina: Protobuf, OpenAPI, vectores, escenarios | Implementes o pruebes contratos |

## 3. Precedencia ante conflictos

De mayor a menor autoridad:

1. `10_SECURITY_PRIVACY.md` — ningún requisito, ADR o conveniencia puede debilitar un control de seguridad o privacidad.
2. Contratos máquina en `contracts/` (`.proto`, OpenAPI) para la **forma** de los datos; `07`, `08` y `09` para su **semántica**. Si forma y semántica discrepan, detener.
3. `03_REQUIREMENTS.md`.
4. `05_DOMAIN_MODEL.md` y `06_IDENTITY_AUTH.md`.
5. `04_ARCHITECTURE.md` y ADR con estado **Aceptado**.
6. `17_CONFIGURATION.md` (valores) y `11_DATA_STORAGE.md` (esquema).
7. `22_TRACKING_ENGINE.md`, `23_MOBILITY.md`, `24_HISTORY_ANALYTICS.md`, `25_EDGE_DEVICES.md` y `26_CORE_AND_BINDINGS.md` (motor; comparten rango con `03` salvo que regulen un control de seguridad, que cae bajo `10`).
8. `19_ENGINEERING_STANDARDS.md`, `20_AGENT_PROTOCOL.md`, `18_TESTING_STRATEGY.md`.
9. `21_ROADMAP.md` (secuencia; no define comportamiento).
10. Guías (`12`–`16`) en lo que no contradiga lo anterior.

Un documento de menor rango que contradiga a uno superior es un defecto documental: se corrige el de menor rango mediante PR.

## 4. Convenciones normativas

### 4.1 Palabras clave

| Español | Equivalente RFC 2119 | Significado |
|---|---|---|
| **DEBE** / **OBLIGATORIO** | MUST | Requisito absoluto |
| **NO DEBE** / **PROHIBIDO** | MUST NOT | Prohibición absoluta |
| **DEBERÍA** / **RECOMENDADO** | SHOULD | Puede omitirse solo con justificación escrita en el PR o un ADR |
| **NO DEBERÍA** | SHOULD NOT | Puede hacerse solo con justificación escrita |
| **PUEDE** / **OPCIONAL** | MAY | Libre elección |

En `03_REQUIREMENTS.md` la prioridad usa MUST / SHOULD / COULD:

- **MUST**: obligatorio para cerrar la fase indicada.
- **SHOULD**: esperado en la fase; aplazarlo requiere un ADR o una nota en `PROGRESS.md` aprobada por el propietario.
- **COULD**: deseable; solo se implementa si su fase está activa y el propietario lo prioriza.

### 4.2 Identificadores

- Requisitos: `FR-<ÁREA>-<NNN>` y `NFR-<ÁREA>-<NNN>`. Son estables: nunca se reutilizan ni se renumeran. Un requisito retirado se marca `RETIRADO` y conserva su ID.
- Tareas de roadmap: `T-F<fase>-<NN>` (por ejemplo `T-F4-07`) para las fases F, y `T-E<fase>-<NN>` (por ejemplo `T-E2-05`) para las fases E.
- Escenarios: `SCN-<NNN>`. Vectores de prueba: `<GRUPO>-<V|I|-<NNN>` (V = válido, I = inválido, y los grupos del motor añaden sufijo descriptivo).
- ADR: `ADR-<NNNN>`.

### 4.3 Fases

`F0`–`F12` conducen a **v1.0 GA**. `F13`–`F16` son pistas posteriores a GA con criterios de entrada propios.

Las fases `E0`–`E9` son la **pista del motor de ubicación y movilidad** (`22`–`26`, ADR-0021). Se ejecutan según las dependencias de `21_ROADMAP.md` §18 y conviven con las fases F que comparten crates:

- **E0** es el único PR (o serie de PR) de adopción documental y de contratos, sin código de producción.
- **E1–E9** comparten crates con F, así que una tarea E no puede tocar un crate que una tarea F en curso posea (`20` §11).
- Los cambios de contrato de la pista E deben entrar **antes** de la congelación del contrato v1 en T-F5-13 (`08` §11).

### 4.4 Idioma

- Documentación en español.
- Identificadores de código, nombres de API, campos, enums, métricas, variables de entorno y mensajes de error dirigidos a máquinas: **inglés**.
- Mensajes de error legibles (`message`, `detail`): inglés, breves, sin datos sensibles.

### 4.5 Formatos canónicos

- Tiempo: UTC. JSON usa RFC 3339 con milisegundos y sufijo `Z` (`2026-10-11T21:04:05.123Z`). Protobuf usa `google.protobuf.Timestamp`.
- Coordenadas: grados decimales WGS 84 (`double`). Distancias en metros, velocidades en m/s, rumbo en grados [0, 360).
- Duraciones en nombres de campo y parámetros llevan unidad explícita: `_ms`, `_s` / `_seconds`, `_m`, `_mps`, `_deg`.
- IDs públicos: prefijo + `_` + UUIDv7 en hexadecimal minúscula sin guiones (ver `05_DOMAIN_MODEL.md` §2).

### 4.6 Ejemplos de código

Todo bloque de código en `docs/` es **normativo en su forma** (firmas, nombres, campos) salvo que esté rotulado `// ILUSTRATIVO`. Los bloques normativos deben compilar una vez implementado el módulo correspondiente; si no compilan, se corrige el documento en el mismo PR.

## 5. Cómo se cambia la documentación

1. Abrir PR que modifique el/los documentos y contratos afectados.
2. Si el cambio afecta una decisión registrada, crear un ADR nuevo que **sustituya** al anterior (no editar ADR aceptados salvo erratas).
3. Actualizar `CHANGELOG.md` (sección *Unreleased*, categoría `Docs` o la que aplique).
4. Si cambia un requisito: actualizar `TRACEABILITY.md`.
5. Si cambia un contrato máquina después de **M2** (ver `21_ROADMAP.md`), los checks de compatibilidad (`buf breaking`, `oasdiff`) deben pasar o el cambio debe ir a una nueva versión mayor del contrato.

## 6. Estado de este paquete

Las cifras de rendimiento son **objetivos a medir**, nunca resultados (P-6). Nada de lo descrito está implementado, auditado ni probado a escala hasta que `PROGRESS.md` y la evidencia de CI lo demuestren. El paquete documental v0.2.0 sigue vigente en todo lo que la pista E no modifica; la v0.3.0 añade los documentos `22`–`26`, los ADR-0021..0032 y los requisitos con fase `E`.
