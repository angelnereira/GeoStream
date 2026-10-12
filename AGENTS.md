# AGENTS.md — Instrucciones obligatorias para agentes de código

Este repositorio está **gobernado por la documentación**. `docs/` y `contracts/` dictan requisitos, reglas, parámetros y validaciones; el código los cumple.

## Antes de tocar nada
1. Lee `docs/00_INDEX.md` (gobierno, precedencia, convenciones).
2. Lee `docs/20_AGENT_PROTOCOL.md` (tu ciclo de trabajo) y `docs/19_ENGINEERING_STANDARDS.md` (reglas de código y Definition of Done).
3. Lee `docs/PROGRESS.md` y toma la siguiente tarea de `docs/21_ROADMAP.md` según las reglas de selección.
4. Lee completos los documentos que la tarea indica y los requisitos que cita en `docs/03_REQUIREMENTS.md`.

## Reglas que no se negocian
- Si dos documentos se contradicen, o falta información, o necesitas cambiar un contrato, un permiso, una política de retención, un parámetro de seguridad o añadir una dependencia: **detente** y crea una solicitud de decisión (`docs/templates/DECISION_REQUEST.md`). No adivines.
- La identidad sale solo de credenciales verificadas; nunca de IDs del payload. Toda consulta filtra por `tenant_id`.
- Nunca escribas coordenadas, tokens, tickets, API keys, secretos ni códigos de invitación en logs, métricas, trazas o errores.
- Compartir ubicación está apagado por defecto y solo el propio subject lo activa.
- Los nombres de campos, rutas, códigos de error y mensajes salen de `contracts/`; los números configurables salen de `docs/17_CONFIGURATION.md`.
- No inventes APIs de crates o paquetes: verifica en la versión fijada.
- Solo dependencias de la lista aprobada (`docs/19_ENGINEERING_STANDARDS.md` §6).
- No desactives pruebas ni checks, no bajes umbrales, no uses `--no-verify`.
- No declares una tarea terminada sin cumplir la Definition of Done y sin el informe con comandos y resultados **reales** (`docs/templates/TASK_REPORT.md`).

## Al terminar cada tarea
Actualiza, en el mismo PR: documentación y contratos afectados (si la tarea lo autoriza), `CHANGELOG.md`, `docs/TRACEABILITY.md` y `docs/PROGRESS.md`.

> Claude Code lee este archivo directamente cuando no existe un `CLAUDE.md`. Si se crea un `CLAUDE.md`, debe contener la línea `@AGENTS.md` para seguir cargándolo.
