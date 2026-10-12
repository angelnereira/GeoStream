# Informe de tarea

> Se pega en la descripción del PR. Todo lo que se afirme como ejecutado debe haberse ejecutado.

## Tarea
- ID: T-Fx-NN — título
- Rama: `<tipo>/T-Fx-NN-…`
- Agente / autor:

## Requisitos cubiertos
| ID | Cómo se cubre | Pruebas (`covers:`) |
|---|---|---|
| FR-… | | |

## Cambios
- Archivos/crates tocados:
- Contratos tocados (si alguno, enlazar la decisión que lo autoriza):
- Documentación actualizada:
- Dependencias nuevas (si alguna, enlazar ADR):

## Verificación ejecutada
| Comando | Resultado |
|---|---|
| `cargo fmt --check` | |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | |
| `cargo test --workspace` | N pruebas, 0 fallos |
| `buf lint` / Redocly | |
| checks de `tools/` | |
| (SDK) `flutter analyze` / `flutter test` | |
| (si aplica) escenarios / benchmarks | métricas medidas y entorno |

## Desviaciones y riesgos
Qué no se pudo verificar y por qué; supuestos; riesgos abiertos.

## Pendiente
Trabajo que queda fuera de esta tarea (con issue/tarea).

## Checklist de Definition of Done (`19` §1)
- [ ] Requisitos implementados sin comportamiento no especificado
- [ ] Pruebas con `covers:` para cada criterio de aceptación
- [ ] CI verde
- [ ] Documentación y contratos actualizados
- [ ] `CHANGELOG.md`, `TRACEABILITY.md`, `PROGRESS.md` actualizados
- [ ] Sin dependencias no aprobadas
- [ ] Revisión de `10` declarada si aplica
