# ADR-0001: Desarrollo gobernado por la documentación

- **Estado:** Aceptado (baseline v0.2.0)
- **Fecha:** 2026-10-11
- **Responsable:** propietario del proyecto
- **Requisitos afectados:** Todos (gobierno)

## Contexto

El proyecto se construirá en gran parte con agentes de código. Sin una fuente de verdad explícita, distintos agentes producen piezas técnicamente correctas pero incompatibles, e improvisan decisiones de seguridad y contrato.

## Opciones consideradas

1. Código como fuente de verdad, documentación descriptiva.
2. Documentación y contratos máquina como fuente de verdad normativa, con precedencia explícita y checks de CI.
3. Solo contratos máquina, sin documentación normativa.

## Decisión

Opción 2. `docs/` y `contracts/` son normativos con la precedencia de `00_INDEX.md` §3. Los cambios de comportamiento actualizan documentación, contratos y pruebas en el mismo PR. Las contradicciones detienen el trabajo (`20_AGENT_PROTOCOL.md` §6). CI verifica la consistencia entre catálogos (errores, configuración, requisitos, matriz de autorización).

## Consecuencias

+ Agentes y humanos trabajan contra el mismo contrato; menos retrabajo; trazabilidad requisito→prueba.
- Mayor coste de cambio: cada decisión requiere actualizar documentos. Riesgo de documentos desactualizados mitigado por checks de CI.

## Seguridad, privacidad y operación

La seguridad (`10`) tiene la máxima precedencia: ningún requisito puede debilitarla.

## Referencias

`00_INDEX.md`, `20_AGENT_PROTOCOL.md`
