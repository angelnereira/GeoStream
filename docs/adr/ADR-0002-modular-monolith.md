# ADR-0002: Monolito modular en un binario con roles

- **Estado:** Aceptado (baseline v0.2.0)
- **Fecha:** 2026-10-11
- **Responsable:** propietario del proyecto
- **Requisitos afectados:** FR-OPS-001, NFR-SCALE-001, NFR-COST-001

## Contexto

Hay que separar dominios (auth, sesiones, stream, worker) y permitir escalar, pero no existe evidencia de carga que justifique microservicios, y el equipo es pequeño.

## Opciones consideradas

1. Microservicios desde el inicio.
2. Monolito modular Rust (crates con reglas de dependencia) en un único binario con roles `api`, `stream`, `worker` activables por proceso.
3. Arquitectura serverless.

## Decisión

Opción 2. Un binario `geostream` con `serve --roles`. Los crates son módulos, no servicios. Separar procesos por rol es configuración; separar en servicios distintos requiere evidencia y un ADR nuevo.

## Consecuencias

+ Despliegue y transacciones simples; bajo coste operativo; escalado por rol posible sin cambiar código.
- Exige disciplina de dependencias (check de CI `04` §4.1).

## Seguridad, privacidad y operación

Superficie de ataque única pero menor; los roles comparten configuración validada.

## Referencias

`04_ARCHITECTURE.md` §4–§5
