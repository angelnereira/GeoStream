# 01 — Visión, alcance y casos de uso

**ID:** GS-VIS-001 · **Versión:** 0.2.0

## 1. Resumen

GeoStream es una **plataforma de infraestructura de ubicación en tiempo real** para que otras aplicaciones y sistemas capturen, compartan, distribuyan y consuman la ubicación de dispositivos de forma autorizada, con privacidad por defecto. Se compone de:

- un **servidor** en Rust (binario único con roles API, stream y worker);
- un **plano de control REST** (sesiones, miembros, invitaciones, compartición, tokens, webhooks);
- un **protocolo de streaming** sobre WebSocket seguro, versionado y definido en Protobuf;
- un **SDK Flutter** de primera clase (Android, iOS; web sin background);
- **adaptadores de mapas** opcionales y desacoplados (MapLibre primero, Google Maps después);
- **integraciones** para backends: webhooks firmados, consumidores de stream del lado servidor, ingesta HTTP.

GeoStream **no es** un proveedor de mapas, ni un sistema de navegación, ni una plataforma de despacho. Es el componente de ubicación que esas plataformas integran.

## 2. Problema

Cada producto que necesita "ver dónde está alguien" (transporte, delivery, logística, seguridad, familia, eventos, IoT) reconstruye la misma infraestructura: captura móvil con permisos y batería, canal en tiempo real, autorización por sesión, revocación, reconexión, presencia, privacidad y acoplamiento a un proveedor de mapas. El resultado suele ser frágil, caro y poco respetuoso con la privacidad.

## 3. Propuesta de valor

1. **Contrato estable** para publicar y consumir ubicación en tiempo real, independiente del proveedor de mapas.
2. **Privacidad por diseño**: compartir está apagado por defecto; sin historial por defecto; la última ubicación se borra al dejar de compartir.
3. **Integración en horas, no semanas**: el backend del integrador emite tokens con una llamada; el SDK hace el resto.
4. **Despliegue simple**: un binario + PostgreSQL (+ Redis/Valkey al escalar).
5. **Honestidad técnica**: semántica latest-state explícita, límites de background documentados, cifras medidas antes de prometerse.

## 4. Objetivos (G)

- **G-1** Compartir y consumir ubicación en tiempo real entre participantes autorizados, en sesiones 1-a-1, 1-a-N y N-a-N.
- **G-2** Control explícito y revocable por parte de cada persona sobre si comparte su ubicación.
- **G-3** Revocación efectiva de lectura y publicación sin esperar a que expiren credenciales.
- **G-4** Corrección ante pérdida de red, duplicados, retrasos, reconexiones y relojes desfasados.
- **G-5** SDK Flutter con API tipada, estados observables y errores tipados.
- **G-6** Integración con sistemas terceros: tokens server-to-server, webhooks, stream del lado servidor, ingesta HTTP.
- **G-7** Escalado horizontal sin cambiar el protocolo público.
- **G-8** Operación observable sin exponer coordenadas.
- **G-9** Mapas, geocodificación y rutas como adaptadores reemplazables.
- **G-10** Geocercas con eventos de entrada/salida (v1.0).

## 5. No objetivos

- **NG-1** Reemplazar proveedores cartográficos (Google, Mapbox, HERE, OSM).
- **NG-2** Garantizar precisión: depende del dispositivo, entorno y sistema operativo.
- **NG-3** Garantizar ejecución indefinida en segundo plano: Android e iOS lo restringen.
- **NG-4** Despacho, matching, tarifas, pagos, pedidos o navegación turn-by-turn.
- **NG-5** Historial de trayectorias por defecto (existe como opción post-GA, F13).
- **NG-6** Exigir Kubernetes, Kafka, P2P, Tile38, H3 o map-matching en cualquier instalación.
- **NG-7** Prometer latencia, capacidad o disponibilidad sin medición en un entorno definido.
- **NG-8** Acceso anónimo a ubicaciones. Toda lectura requiere un principal autenticado y autorizado.

## 6. Actores

| Actor | Descripción |
|---|---|
| **Integrador** | Equipo que integra GeoStream en su producto (backend + app). |
| **Backend del integrador** | Servicio del integrador que autentica a sus usuarios y obtiene tokens de GeoStream con una API key. |
| **Subject** | Persona o entidad del integrador identificada por `external_subject_id`. El consentimiento es del subject. |
| **Dispositivo** | Instalación concreta de una app en un teléfono, navegador o equipo. Pertenece a un subject. |
| **Propietario de sesión** | Subject que crea la sesión (si la crea un dispositivo). |
| **Miembro** | Subject con membresía activa en una sesión, con capacidades `publish`, `read`, `invite`, `manage`. |
| **Aplicación (principal)** | Credencial server-to-server del integrador dentro de un tenant. |
| **Operador** | Quien despliega y opera GeoStream. Observa salud sin acceso a coordenadas. |
| **Administrador de tenant** | Gestiona tenants, aplicaciones, API keys y políticas mediante la CLI admin. |

## 7. Casos de uso

Cada caso se convierte en uno o más escenarios ejecutables (`contracts/scenarios/`).

### UC-01 — Seguimiento de un viaje (ride-hailing)
El backend del integrador crea una sesión `one_to_one` al asignar conductor y pasajero, agrega ambos subjects como miembros, y los dispositivos activan compartir. Ambos ven la ubicación del otro. Al terminar el viaje el backend cierra la sesión: se borran snapshots y terminan las suscripciones.

### UC-02 — Seguimiento de un pedido (delivery)
Sesión `one_to_many`: el repartidor publica; el cliente (y el backend con `locations:read`) leen. El cliente puede ser un visitante web con un token emitido para un subject invitado.

### UC-03 — Compartir entre familia o equipo
Un dispositivo crea una sesión `many_to_many` de larga duración (dentro de la política del tenant) e invita a otros con un código. Cada miembro decide individualmente si comparte. Dejar de compartir no saca al miembro de la sesión.

### UC-04 — Flota
Sesión propiedad de la aplicación con muchos publicadores (conductores con `publish`, sin `read`) y el backend como consumidor de stream del lado servidor. Los conductores no ven a los demás.

### UC-05 — Evento temporal
Sesión con expiración corta; los asistentes se unen por invitación multiuso; la sesión expira sola.

### UC-06 — Consulta de última ubicación autorizada
Un miembro con `read` (o el backend con `locations:read`) consulta el snapshot REST de una sesión.

### UC-07 — Llegada a destino (geocerca)
Una geocerca asociada a la sesión de un pedido dispara `geofence.entered` por webhook cuando el repartidor llega.

### UC-08 — Dispositivo IoT o gateway
Un equipo sin app móvil publica por ingesta HTTP en lotes con un token de dispositivo (MQTT nativo es pista F14).

## 8. Principios de diseño

- **P-1 Privacidad por defecto.** Compartir apagado al unirse; sin historial; snapshot efímero con TTL; borrado al dejar de compartir; coordenadas nunca en logs.
- **P-2 La identidad sale de las credenciales.** Nunca de IDs del payload.
- **P-3 Ubicación ≠ mapa.** El núcleo funciona sin ningún proveedor de mapas.
- **P-4 Semántica latest-state.** La ubicación es estado reciente best-effort; los eventos de control son confiables e idempotentes.
- **P-5 Simplicidad operativa.** Monolito modular, un binario, dependencias mínimas.
- **P-6 Afirmaciones medidas.** Distinguir objetivo, resultado medido y SLA.
- **P-7 La documentación gobierna.** Ver `00_INDEX.md`.

## 9. Definición de "producto listo" (v1.0 GA)

GeoStream v1.0 está listo cuando se cumplen **todos** los criterios de salida de F0–F12 en `21_ROADMAP.md`, entre ellos:

- Todos los requisitos MUST con fase ≤ F12 implementados y trazados a pruebas que pasan.
- SDK Flutter publicado para Android e iOS con app de ejemplo y pruebas en dispositivos reales.
- Webhooks, consumidor de stream del lado servidor e ingesta HTTP disponibles y documentados.
- Adaptador de mapas MapLibre publicado; Google Maps como adaptador adicional.
- Geocercas con eventos por stream y webhook.
- Benchmarks reproducibles publicados con resultados medidos frente a los objetivos NFR.
- Checklist de seguridad de liberación completo, runbooks y restauración de backups probada.

## 10. Cambios respecto a la propuesta fundacional v0.1.0

Resumen; detalle en `CHANGELOG.md` y ADR referenciados.

- Modelo de identidad definido: API keys de aplicación + tokens de dispositivo emitidos por GeoStream + tickets de stream de un solo uso (ADR-0006, ADR-0007).
- Nueva entidad **Subject** por encima de **Device**; el consentimiento es del subject y hay un solo dispositivo publicador por membresía (ADR-0006, `05_DOMAIN_MODEL.md`).
- `location.publish` (entrante) y `location.update` (saliente) separados; `location.update` incluye `member_id` y `received_at`.
- Secuencia por instalación de dispositivo con *sequence floor* del servidor (ADR-0008).
- Revocación con caché de autorización de TTL acotado + invalidación por bus (ADR-0009).
- Cola offline por defecto = solo el último punto (ADR-0014).
- Presencia separada en `sharing`, `connectivity` (solo si comparte) y `last_location_at`.
- Modos `broadcast` e `hybrid` retirados: `broadcast` es `one_to_many` con límite alto; P2P es una pista de transporte (F16).
- Geocercas en proceso (R-tree) por defecto; Tile38/H3 diferidos (ADR-0016).
- Primer adaptador de mapas: MapLibre (ADR-0013).
