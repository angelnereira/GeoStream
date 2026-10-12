# ADR-0003: Backend en Rust con Tokio, Axum, sqlx y Protobuf

- **Estado:** Aceptado (baseline v0.2.0)
- **Fecha:** 2026-10-11
- **Responsable:** propietario del proyecto
- **Requisitos afectados:** NFR-PERF-*, NFR-SEC-*, NFR-MAINT-*

## Contexto

El servidor mantiene muchas conexiones largas con baja latencia y debe ser seguro en memoria. El propietario domina Rust y lo usa como lenguaje principal.

## Opciones consideradas

1. Rust (Tokio, Axum, sqlx, prost).
2. Go.
3. Node.js/TypeScript.

## Decisión

Rust estable fijado por `rust-toolchain.toml`, edición 2024, `#![forbid(unsafe_code)]`. Lista cerrada de dependencias aprobadas en `19_ENGINEERING_STANDARDS.md` §6.2; añadir otra requiere ADR.

## Consecuencias

+ Rendimiento y seguridad de memoria; un solo lenguaje en el servidor; tipos fuertes para el dominio.
- Curva de aprendizaje para contribuyentes; tiempos de compilación.

## Seguridad, privacidad y operación

`cargo deny` y `cargo audit` en CI; criptografía solo de crates revisados.

## Referencias

`19_ENGINEERING_STANDARDS.md` §3, §6
