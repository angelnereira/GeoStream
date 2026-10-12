# GeoStream — Brechas y plan de completitud

**Versión:** 0.1 (borrador de análisis) · **Fecha:** 2026-10-12 · **Base:** repositorio documental `v0.2.0`
**Estado de este documento:** propuesta. No modifica ningún documento normativo del repositorio. Para adoptar un punto hay que convertirlo en requisito (`docs/03`), tarea (`docs/21`) y, si procede, ADR.

---

## 1. Propósito

Responder con precisión: **¿qué le falta a GeoStream para ser un servicio reutilizable, fácil de integrar y completo?** Cada brecha indica qué existe hoy en el diseño, qué habría que integrar, cómo se sabría que está resuelta y cuánto cuesta aproximadamente.

**Lo que este documento no afirma.** Todo lo que sigue se contrasta contra los documentos del repositorio, no contra código: nada está compilado ni medido. Los esfuerzos son estimaciones sin base empírica (S ≤ 1 semana, M = 1–3 semanas, L > 3 semanas, para una persona con un agente de código).

## 2. Resumen priorizado

| ID | Brecha | Prioridad | Estado hoy en el diseño | Esfuerzo |
|---|---|---|---|---|
| G-01 | Spike de validación antes de congelar contratos | P0 | No está en la hoja de ruta | S |
| G-02 | Herramientas de verificación dentro del repositorio y en CI | P0 | Fuera del repositorio (scratchpad) | S |
| G-03 | Verificar dependencias del SDK Flutter | P0 | Diferido a T-F5-01 y T-F9-02 | S |
| G-04 | Licencia del proyecto | P0 | ADR-0019 propuesto, sin decidir | S |
| G-05 | SDK TypeScript (web/Node) y ayudantes de backend | P1 | No existe (solo Flutter) | M–L |
| G-06 | Apps y backend de ejemplo, entorno sandbox | P1 | Solo ejemplos ilustrativos en `15` | M |
| G-07 | Despertar de dispositivos por push (FCM/APNs) | P1 | No figura en el diseño | M |
| G-08 | Medición de uso, cuotas y facturación | P1 | No hay requisito | M |
| G-09 | API de administración y consola web | P1 | CLI; API HTTP es COULD en F15 | M–L |
| G-10 | Consulta y reproducción de eventos de webhook | P1 | Sin orden garantizado ni reproducción | M |
| G-11 | Derechos de acceso/supresión de extremo a extremo | P1 | Parcial (endpoints de F10) | M |
| G-12 | Gestión de claves y secretos con rotación probada | P1 | Parcial (`10`) | S–M |
| G-13 | Pruebas de penetración y revisión legal (DPIA) | P1 | Recomendadas, no planificadas | M |
| G-14 | Evidencia de rendimiento, batería y resiliencia | P1 | Perfiles definidos, no ejecutados | L |
| G-15 | Historial y trayectorias | P2 | F13, COULD | L |
| G-16 | Identidad federada (OIDC/JWKS) | P2 | F15, COULD | M |
| G-17 | Multi-región, residencia de datos y recuperación ante desastres | P2 | Solo respaldo en una región | L |
| G-18 | Cifrado extremo a extremo y P2P | P2 | F16 | L |
| G-19 | MQTT para IoT | P2 | F14 | M |
| G-20 | Núcleo Rust compartido para SDKs nativos | P2 | F15, requiere ADR | L |
| G-21 | Geocercas en dispositivo, permanencia y proximidad | P2 | Fuera de v1 (`14`) | M |
| G-22 | Plausibilidad de ubicación (antifraude básico) | P2 | T-07: fuera de alcance | M |

**Prioridades:** P0 = antes de declarar estables los contratos. P1 = necesarias para que otros sistemas lo adopten con confianza (v1.0 GA o inmediatamente después). P2 = extienden el producto; se hacen cuando haya un integrador que las pida.

---

## 3. P0 — Antes de congelar contratos

### G-01 · Spike de validación
- **Hoy:** los contratos están en `1.0.0-draft.2` y nunca se han ejercitado contra un runtime.
- **Integrar:** 1–2 semanas con un único binario Axum, almacén en memoria, JSON sobre WebSocket y la app Flutter en dos teléfonos Android. Objetivo: confirmar el flujo token → ticket → stream → publicar → recibir, el reconectado y el consumo de batería en segundo plano.
- **Resultado esperado:** lista de cambios a contratos y documentos *antes* de que existan integradores. Se borra o se reabsorbe en F1–F5; no es producción.
- **Aceptación:** informe con latencias observadas, problemas de plataforma encontrados y cambios aplicados a `contracts/`.
- **Esfuerzo:** S–M.

### G-02 · Herramientas de verificación dentro del repositorio
- **Hoy:** el verificador de consistencia, los generadores de vectores/escenarios y el validador de referencia están fuera del repositorio. T-F0-05 espera `tools/`.
- **Integrar:** mover a `tools/`, fijar versiones de `buf`, `@redocly/cli` y `oasdiff`, y ejecutar en CI desde F0 (lint de proto, lint de OpenAPI, ida y vuelta de mensajes, vectores, enlaces, tablas, diagramas).
- **Aceptación:** `make verify` (o equivalente) falla ante cualquier incoherencia entre documentos y contratos.
- **Esfuerzo:** S.

### G-03 · Dependencias del SDK Flutter sin verificar
- **Hoy:** no se pudo confirmar nombre, versión ni licencia de `geolocator` y `maplibre_gl` (pub.dev bloqueó la consulta).
- **Integrar:** verificar manualmente en pub.dev (versión, licencia, plataformas, mantenimiento, comportamiento en segundo plano) y registrar el resultado en `19` y ADR-0012/0013.
- **Aceptación:** tabla de dependencias aprobadas con versión fijada y licencia confirmada.
- **Esfuerzo:** S.

### G-04 · Licencia
- **Hoy:** ADR-0019 propuesto, sin decisión del propietario.
- **Integrar:** decidir licencia del servidor, del SDK y de los contratos (pueden ser distintas). Condiciona la publicación de `geostream` en pub.dev.
- **Aceptación:** ADR-0019 aceptado y archivos `LICENSE` en cada paquete.
- **Esfuerzo:** S (decisión, no trabajo).

---

## 4. P1 — Para que otros sistemas lo adopten

### G-05 · SDK TypeScript y ayudantes de backend
- **Hoy:** solo existe el SDK Flutter. Un integrador en web o Node debe implementar REST y el codec Protobuf a mano; `gs.v1.json` está pensado solo para pruebas.
- **Integrar:** dos paquetes npm generados/derivados de `contracts/`:
  1. `@geostream/client`: stream con `gs.v1.proto`, reconexión, secuencias y estados (misma máquina de estados que Dart).
  2. `@geostream/server`: emisión de tokens (`POST /v1/tokens`), verificación de firma de webhooks, cliente REST tipado y consumidor de stream del lado servidor.
- **Aceptación:** pasa los mismos vectores y escenarios que el SDK Dart; ejemplo mínimo de integración en menos de 100 líneas.
- **Esfuerzo:** M (servidor) + M–L (cliente).
- **Nota:** reduce el acoplamiento práctico a Flutter sin tocar el servidor.

### G-06 · Aplicaciones de ejemplo y sandbox
- **Hoy:** `15_INTEGRATION_GUIDE.md` contiene fragmentos ilustrativos (Node.js) que no se ejecutan ni se prueban.
- **Integrar:** `examples/` con (a) backend de ejemplo que emite tokens y recibe webhooks, (b) app Flutter de ejemplo con mapa, (c) simulador de dispositivos. Un tenant `sandbox` con límites reducidos y datos efímeros.
- **Aceptación:** los ejemplos corren en CI contra un servidor real y están enlazados desde la guía.
- **Esfuerzo:** M.

### G-07 · Despertar de dispositivos por push
- **Hoy:** no hay mecanismo para pedir a un dispositivo que empiece a publicar cuando su app está en segundo plano o cerrada. No aparece FCM/APNs en los documentos.
- **Integrar (propuesta):** decidir si el servidor envía pushes silenciosos o si lo hace el backend del integrador a partir de un webhook (p. ej. `member.share_requested`). La segunda opción evita guardar credenciales de push en GeoStream y mantiene el servicio sin dependencias externas.
- **Aceptación:** escenario donde un miembro solicita ubicación a otro y el dispositivo reanuda la publicación; documentar límites de iOS (los pushes silenciosos no están garantizados).
- **Esfuerzo:** M. Requiere ADR.

### G-08 · Medición de uso, cuotas y facturación
- **Hoy:** existen límites por política (geocercas, miembros, tasas), pero no hay requisito de medición de uso ni de facturación, aunque el concepto de tenant lo sugiere.
- **Integrar:** definir si GeoStream factura o solo mide. Mínimo recomendable: contadores por tenant (conexiones pico, mensajes publicados, entregas, webhooks, minutos de sesión) expuestos por API y exportables; sin etiquetar métricas Prometheus con tenant (regla de cardinalidad vigente), por lo que irían a PostgreSQL agregados por hora.
- **Aceptación:** informe de uso por tenant y período reproducible; cuotas con rechazo claro (`RATE_LIMITED` o un código nuevo) y evento de aviso.
- **Esfuerzo:** M. Si es solo uso interno, declarar fuera de alcance en `01`.

### G-09 · API de administración y consola
- **Hoy:** administración solo por CLI; la API HTTP es FR-ADM-004 (COULD, F15).
- **Integrar:** subir FR-ADM-004 a MUST antes de GA si habrá más de un tenant operado por terceros. Consola web mínima (tenants, aplicaciones, claves, dispositivos, sesiones, entregas de webhook, auditoría) que consuma esa API.
- **Aceptación:** todo lo que hace la CLI se puede hacer por API con autenticación de operador separada de las API keys de aplicación.
- **Esfuerzo:** M (API) + M (consola).

### G-10 · Consulta y reproducción de eventos
- **Hoy:** los webhooks no garantizan orden; el integrador debe ser idempotente por `id`.
- **Integrar:** `GET /v1/events` paginado y filtrable (con retención corta, p. ej. 7 días) y reintento manual de una entrega; número de secuencia por recurso para reordenar. Permite recuperarse de caídas largas del receptor sin pedir soporte.
- **Aceptación:** receptor caído 1 h recupera todos los eventos perdidos sin duplicar efectos.
- **Esfuerzo:** M.

### G-11 · Derechos de acceso y supresión de extremo a extremo
- **Hoy:** el integrador debe atender derechos de acceso/supresión; hay endpoints previstos en F10.
- **Integrar:** definir qué datos tiene GeoStream por sujeto (identificador externo, dispositivos, membresías, auditoría), exportarlos y suprimirlos por sujeto, con propagación a webhooks pendientes y a la auditoría. Sin historial (antes de F13) el alcance es pequeño; con F13 crece.
- **Aceptación:** supresión de un sujeto elimina datos personales en menos de un plazo documentado y deja constancia sin datos personales.
- **Esfuerzo:** M.

### G-12 · Gestión de claves y secretos
- **Hoy:** las claves de firma JWT residen en el sistema de archivos o en un gestor de secretos; hay rotación definida.
- **Integrar:** procedimiento verificado de rotación (claves de firma, secretos de webhook, claves de aplicación), integración documentada con el gestor de secretos del entorno de despliegue y ensayo de compromiso de clave (runbook).
- **Aceptación:** rotación en caliente sin cortar streams, probada en CI y en un ensayo manual.
- **Esfuerzo:** S–M.

### G-13 · Pruebas de penetración y revisión legal
- **Hoy:** la lista de verificación de lanzamiento recomienda DPIA; el documento de privacidad orienta sobre la Ley 81 de 2019 y el Decreto 285 de 2021 pero no es asesoría legal.
- **Integrar:** prueba de penetración externa del plano de control y del stream; revisión legal de textos de consentimiento, retención y contratos de tratamiento con integradores.
- **Aceptación:** informe sin hallazgos críticos abiertos; DPIA firmada.
- **Esfuerzo:** M (calendario, no desarrollo).

### G-14 · Evidencia de rendimiento, batería y resiliencia
- **Hoy:** los objetivos NFR-PERF y los perfiles P-S, P-M, P-L, P-FLEET y P-STORM están definidos; ninguno se ha ejecutado. La lista de verificación de dispositivos tampoco.
- **Integrar:** ejecutar las cargas con `gs-loadgen`, ensayos de caos (caída de Valkey, caída del bus, reinicio de nodo, partición de red), pruebas de restauración de respaldos (RPO ≤ 15 min, RTO ≤ 1 h) y mediciones de batería en al menos un Android y un iPhone reales con distintos perfiles de muestreo.
- **Aceptación:** informe publicado en el repositorio; los objetivos se mantienen, se ajustan con datos o se retiran. Solo entonces se publica un SLO.
- **Esfuerzo:** L.

---

## 5. P2 — Extensiones

| ID | Brecha | Qué integrar | Condición para hacerlo |
|---|---|---|---|
| G-15 | Historial y trayectorias | FR-HIS-001..004: historial opt-in con retención por política, cola offline acumulada (hoy `latestOnly`), consulta paginada, suavizado y map-matching. Cambia el modelo de privacidad: hay que actualizar NFR-PRIV-001, consentimiento y supresión (G-11). | Un integrador necesita prueba de ruta o analítica. |
| G-16 | Identidad federada | FR-AUTH-009: aceptar tokens firmados por el integrador con JWKS u OIDC, sin necesitar llamar a `POST /v1/tokens` por sesión. | Integrador con muchos usuarios y quiere quitar su endpoint de token. |
| G-17 | Multi-región y DR | Residencia de datos por tenant, réplica de PostgreSQL entre regiones, conmutación del plano de control; el almacén efímero se reconstruye (por diseño). Requiere ADR y revisión de la topología T1–T4. | Requisito contractual de disponibilidad o de residencia. |
| G-18 | E2EE y P2P | F16: cifrado de coordenadas entre miembros. Rompe geocercas y validación en el servidor (T-22 es la limitación aceptada), por lo que debe ser opcional por sesión. | Casos de datos muy sensibles. |
| G-19 | MQTT | F14: ingesta por MQTT para dispositivos IoT con poca capacidad, mapeada a las mismas reglas de validación. | Flota de dispositivos que no puede mantener un WebSocket. |
| G-20 | Núcleo Rust compartido | F15: extraer codec, validación local y máquina de estados a una biblioteca Rust con FFI para SDKs nativos (Swift/Kotlin) y, si compensa, también para Dart. Requiere ADR; mientras tanto la paridad entre SDKs se asegura con los mismos vectores. | Existen 3 o más SDKs y la paridad cuesta más que el núcleo. |
| G-21 | Geocercas avanzadas | Evaluación en el dispositivo (menos batería), evento de permanencia, consultas de proximidad entre miembros. Hoy fuera de alcance (`14` §1). | Geocercas masivas o necesidad de ahorro de batería. |
| G-22 | Plausibilidad | Hoy T-07 declara fuera de alcance garantizar ubicación auténtica; se transporta `is_mock` si la plataforma lo da. Opción: calcular indicios (velocidad imposible, saltos) y entregarlos como metadato separado, sin modificar la muestra (el servidor no debe alterar posición ni calidad). Es un cambio de contrato. | Casos con incentivo a falsificar ubicación (flotas, reparto con pago). |

---

## 6. Lo que se recomienda **no** integrar

Para mantener un servicio reutilizable y acotado:

- **Despacho y asignación** de conductores o pedidos: pertenece al integrador.
- **Navegación y cálculo de rutas/ETA propios:** se consumen de proveedores a través de los adaptadores del doc 13, respetando sus términos.
- **Pagos y cobros al usuario final.**
- **Perfiles de usuario, nombres, fotos, chat:** GeoStream solo conoce un identificador externo opaco.
- **Mapas propios:** el servidor no depende de ningún SDK de mapas.

## 7. Orden recomendado

1. **Ahora:** G-04 (decisión), G-03 y G-02, y planificar G-01.
2. **Spike (G-01)**, y con sus resultados, ajustar contratos y arrancar F0–F1.
3. **Durante F1–F9:** G-12, G-10 y G-11 se integran en las fases afines; G-07 se decide antes de congelar el protocolo de sesión.
4. **Antes de GA (F12):** G-14 y G-13 son puertas de salida; G-05, G-06 y G-09 deciden si el servicio es realmente reutilizable por terceros; G-08 si es un servicio que se cobra.
5. **Después de GA:** P2 según demanda real, empezando por G-15 y G-16.

## 8. Decisiones que corresponden al propietario

| # | Decisión | Afecta a |
|---|---|---|
| D-1 | Licencia (ADR-0019) | G-04, publicación |
| D-2 | ¿Se adopta el spike antes de congelar contratos? | G-01, calendario |
| D-3 | ¿GeoStream se factura o es de uso interno? | G-08, G-09 |
| D-4 | ¿Habrá integradores fuera de Flutter en v1? | G-05 (sube a P0 si sí) |
| D-5 | ¿Se necesita prueba de ruta o historial en el primer integrador? | G-15 (sube a P1 si sí) |
| D-6 | ¿Quién despierta a los dispositivos: servidor o backend del integrador? | G-07 |
| D-7 | ¿Requisitos de residencia de datos o alta disponibilidad contractuales? | G-17 |

## 9. Cómo incorporarlo al repositorio

Si se aprueban brechas, los cambios mínimos son:

1. `docs/03_REQUIREMENTS.md`: nuevos requisitos con ID, fase y criterio de aceptación; subir de nivel FR-ADM-004 o FR-AUTH-009 si se decide.
2. `docs/21_ROADMAP.md`: tareas nuevas con dependencias (spike, `tools/`, SDK TypeScript, ejemplos, medición de uso).
3. `docs/adr/`: ADR para G-07, G-08 y G-20 como mínimo.
4. `docs/TRACEABILITY.md`: filas para los requisitos nuevos.
5. `contracts/`: cambios en OpenAPI o Protobuf solo después del spike; ejecutar `buf lint`, Redocly y los vectores.
6. `docs/01_VISION_SCOPE.md`: actualizar objetivos y no-objetivos según D-3 y D-4.

El verificador de consistencia (G-02) debe pasar con 0 errores antes de cerrar cualquier cambio.
