# ADR-0017: Observabilidad con tracing y Prometheus; redacción por tipos

- **Estado:** Aceptado (baseline v0.2.0)
- **Fecha:** 2026-10-11
- **Responsable:** propietario del proyecto
- **Requisitos afectados:** NFR-OBS-001, NFR-OBS-002, FR-PRV-004, NFR-SEC-003

## Contexto

La operación necesita métricas y logs ricos, pero las coordenadas y credenciales no pueden filtrarse por ellos.

## Opciones consideradas

1. Redacción por expresiones regulares en la salida.
2. Redacción por construcción: tipos con `Debug`/`Display` redactados, sin `Serialize` hacia logs, labels de métricas acotados y verificados.
3. Sin logs detallados.

## Decisión

Opción 2, más un verificador de captura de logs sobre la suite de escenarios y una inspección de `/metrics`. Trazas OTLP opcionales con las mismas restricciones.

## Consecuencias

+ La protección no depende de recordar redactar en cada sitio.
- Depuración de coordenadas solo en `dev` con bandera explícita.

## Seguridad, privacidad y operación

Cumple `10` §8.

## Referencias

`16_OPERATIONS.md` §4, `10_SECURITY_PRIVACY.md` §8
