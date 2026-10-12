# GeoStream

**Infraestructura de ubicación en tiempo real, privada por defecto, para integrar en otros sistemas.**

> Estado: **paquete documental v0.2.0** — repositorio gobernado por la documentación, listo para iniciar la implementación desde cero. **Aún no existe código.** Ninguna cifra de rendimiento es un resultado medido.
>
> "GeoStream" es un nombre de trabajo: verificar marcas, dominios y nombres de paquetes antes de publicar (T-F0-08).

## Qué es
- Servidor en Rust (un binario con roles `api`, `stream`, `worker`) sobre PostgreSQL y, al escalar, Redis/Valkey.
- Plano de control REST (OpenAPI 3.1) y protocolo de streaming WebSocket (Protobuf) versionados.
- SDK Flutter (Android, iOS, web sin segundo plano) y adaptadores de mapas opcionales (MapLibre, Google Maps).
- Integraciones para backends: tokens server-to-server, webhooks firmados, consumidores de stream del lado servidor, ingesta HTTP.
- Sesiones 1-a-1, 1-a-N y N-a-N; cada persona decide si comparte; revocación efectiva con plazo acotado; sin historial por defecto.

Qué **no** es: un proveedor de mapas, un navegador turn-by-turn ni una plataforma de despacho.

## Cómo está organizado

```text
AGENTS.md            Reglas obligatorias para agentes de código (punto de entrada)
docs/                Documentación normativa (empieza por docs/00_INDEX.md)
  00_INDEX.md          Gobierno, precedencia y convenciones
  01–02                Visión, alcance, glosario
  03_REQUIREMENTS.md   Requisitos FR/NFR con fase y criterio de aceptación
  04–06                Arquitectura, modelo de dominio, identidad y autorización
  07–09                API REST, protocolo de stream, validación y errores
  10–11                Seguridad y privacidad, datos y almacenamiento
  12–15                SDK Flutter, mapas, geocercas, guía de integración
  16–18                Operación, configuración, pruebas
  19–21                Estándares de ingeniería, protocolo de agentes, hoja de ruta
  adr/                 Decisiones de arquitectura
  templates/           ADR, informe de tarea, solicitud de decisión
  TRACEABILITY.md      Requisito → pruebas → estado
  PROGRESS.md          Bitácora de avance
contracts/           Contratos máquina: proto, OpenAPI, vectores de prueba, escenarios
```

## Orden de lectura
1. `AGENTS.md`
2. `docs/00_INDEX.md`
3. `docs/01_VISION_SCOPE.md` → `docs/03_REQUIREMENTS.md` → `docs/04_ARCHITECTURE.md`
4. El resto según la tarea (`docs/20_AGENT_PROTOCOL.md` §5).

## Cómo empezar la implementación
1. Revisar y, si procede, ajustar los ADR (en especial ADR-0019, licencia, pendiente de decisión).
2. Crear el repositorio en GitHub con este contenido como primer commit.
3. Dar a un agente de código el prompt de `docs/20_AGENT_PROTOCOL.md` §12. Empezará por **F0** (`docs/21_ROADMAP.md`).

## Verificación de contratos

```bash
(cd contracts/proto && buf lint)
npx @redocly/cli lint contracts/openapi/geostream.v1.yaml --config contracts/openapi/redocly.yaml
```

## Matriz de compatibilidad
Se completa en T-F5-01 y se mantiene en cada release (NFR-COMPAT-001).

| Componente | Versión soportada |
|---|---|
| Rust (toolchain fijada) | por definir en T-F0-01 |
| PostgreSQL | ≥ 16 |
| Redis / Valkey | ≥ 7.2 |
| Flutter / Dart | por definir en T-F5-01 |
| Android (`minSdk` / `targetSdk`) | por definir en T-F5-01 |
| iOS mínimo | por definir en T-F7-01 |

## Licencia
Pendiente de decisión (ADR-0019). Hasta entonces, todos los derechos reservados por el propietario.
