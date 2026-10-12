# Bitácora de avance

**ID:** GS-PRG-001 · La mantienen los agentes y desarrolladores en cada PR (`20_AGENT_PROTOCOL.md` §3, §4).

## Estado actual

| Campo | Valor |
|---|---|
| Fase activa | **F0 — Bootstrap del repositorio y CI** (en curso) |
| Último hito alcanzado | — |
| Paquete documental | v0.2.0 (2026-10-11) |
| Decisiones pendientes | ADR-0019 (licencia) — no bloquea la implementación; bloquea publicar paquetes |

## Tareas en curso

| Tarea | Agente / persona | Desde | Crates/paquetes que posee | Notas |
|---|---|---|---|---|
| — | | | | |

## Bloqueos y solicitudes de decisión abiertas

| ID | Tarea | Resumen | Abierta |
|---|---|---|---|
| — | | | |

## Fases

| Fase | Estado | Cierre | Evidencia |
|---|---|---|---|
| F0 | en curso | | |
| F1 | pendiente | | |
| F2 | pendiente | | |
| F3 | pendiente | | |
| F4 (M1) | pendiente | | |
| F5 (M2) | pendiente | | |
| F6 | pendiente | | |
| F7 (M3) | pendiente | | |
| F8 | pendiente | | |
| F9 (M4) | pendiente | | |
| F10 | pendiente | | |
| F11 | pendiente | | |
| F12 (M5) | pendiente | | |

## Registro

Formato de cada entrada (más reciente arriba):

```text
### AAAA-MM-DD — T-Fx-NN — <título>
- Autor/agente:
- Resultado: completada | parcial | bloqueada
- PR:
- Requisitos: …
- Evidencia: enlace a CI / informe
- Notas: desviaciones, decisiones tomadas dentro del margen de 20 §7
```

### 2026-10-12 — T-F0-01 — Workspace Cargo y toolchain
- Autor/agente: Angel Nereira (con agente de código).
- Resultado: parcial — entregable completo y verificado en local; CI aún no existe (T-F0-05), pendiente de PR.
- PR: — (rama `chore/T-F0-01-cargo-workspace`)
- Requisitos: FR-OPS-001 (base; los subcomandos llegan en T-F0-03).
- Evidencia: local, rustc 1.99.0 — `cargo fmt --all --check` sin cambios; `cargo clippy --workspace --all-targets --all-features -- -D warnings` sin avisos; `cargo test --workspace` 0 pruebas, 0 fallos; `cargo metadata` lista 17 paquetes con las dependencias de `04` §4.1.
- Notas: `gs-testkit`, `gs-devclient` y `gs-loadgen` sin dependencias internas (`04` §4.1 no las define). `.editorconfig` y `.gitignore` revisados sin cambios. Versión de paquetes `0.0.0` y `publish = false` hasta ADR-0019.

### 2026-10-11 — Paquete documental v0.2.0
- Autor: diseño inicial.
- Resultado: documentación normativa, ADR, contratos (`buf lint` y Redocly sin errores), vectores y escenarios listos.
- Siguiente paso: T-F0-01.
