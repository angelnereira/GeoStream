# 20 — Protocolo de trabajo para agentes de IA

**ID:** GS-AGT-001 · **Versión:** 0.2.0
Aplica a cualquier agente de código (Claude Code, Codex, OpenCode u otros) y, por extensión, a desarrolladores humanos. Complementa `19_ENGINEERING_STANDARDS.md`.

## 1. Propósito
Que varios agentes puedan construir GeoStream desde un repositorio vacío sin improvisar decisiones, sin construir piezas incompatibles y dejando evidencia verificable de cada paso.

## 2. Arranque de cada sesión de trabajo
1. Leer `AGENTS.md` y `docs/00_INDEX.md`.
2. Leer `docs/PROGRESS.md` para saber qué está hecho, qué está en curso y qué está bloqueado.
3. Elegir la tarea (§3) en `docs/21_ROADMAP.md`.
4. Leer **completos** los documentos que la tarea lista en "Leer", más `03_REQUIREMENTS.md` para los IDs citados.
5. Revisar los ADR relacionados y los contratos (`contracts/`) que la tarea toque.

## 3. Selección de tarea
- Se toma la tarea pendiente de menor fase cuyas dependencias estén cerradas. Dentro de una fase, en el orden listado salvo que la tarea diga "paralelizable".
- No se empieza una fase hasta cumplir los criterios de salida de la anterior, salvo tareas marcadas explícitamente como adelantables en el roadmap.
- Una tarea por rama y por PR.
- Si `PROGRESS.md` marca una tarea "en curso" por otro agente, no se toma.
- Al tomarla, se registra en `PROGRESS.md` (`en curso`, agente, fecha) en el primer commit de la rama.

## 4. Ciclo por tarea

| Paso | Acción | Salida |
|---|---|---|
| 1 | Entender | Lista de requisitos (FR/NFR), invariantes, contratos y parámetros afectados |
| 2 | Detectar conflictos | Si hay contradicción o vacío → §6 (parar) |
| 3 | Planificar | Plan breve en el PR: módulos a tocar, pruebas a escribir, riesgos |
| 4 | Pruebas primero | Pruebas que expresan los criterios de aceptación (fallan inicialmente) con `covers:` |
| 5 | Implementar | Código mínimo que cumple, siguiendo `19` |
| 6 | Verificar | Ejecutar formato, lint, pruebas, contratos y checks de consistencia localmente |
| 7 | Documentar | Actualizar docs/contratos si la tarea lo exige (nunca para "acomodar" una implementación sin decisión), `CHANGELOG.md`, `TRACEABILITY.md`, `PROGRESS.md` |
| 8 | Reportar | Informe `docs/templates/TASK_REPORT.md` en el PR con comandos y resultados **reales** |
| 9 | Cerrar | Solo con CI verde y criterios de aceptación cumplidos |

## 5. Mapa de lectura por área

| Si trabajas en… | Lee además de 00 y 03 |
|---|---|
| `gs-domain` | 05, 09, 17 §4, `contracts/test-vectors/` |
| `gs-protocol` | 08, `contracts/proto/`, `contracts/README.md` |
| `gs-config` | 17 |
| `gs-auth` | 06, 10 §5.6, `contracts/test-vectors/token_claims.json` |
| `gs-store`, migraciones | 11, 05, 10 §6–§7 |
| `gs-ephemeral`, `gs-bus` | 04 §6, 11 §3, ADR-0009, ADR-0010 |
| `gs-service` | 05, 06, 07, 09, 04 §7, 11 §2.1 |
| `gs-api` | 07, 06 §8, 09, OpenAPI |
| `gs-stream` | 08, 04 §8, 09 §4, 06 §8.3 |
| `gs-worker` | 11 §5, 07 §10, 10 §7 |
| `gs-geo` | 14 |
| `gs-observability` | 16 §4, 10 §8 |
| `gs-testkit`, escenarios | 18, `contracts/README.md` §5 |
| SDK Flutter | 12, 08, 09, 06 §2, 17 §5 |
| Adaptadores de mapas | 13 |
| Despliegue | 16, 17, 04 §10 |
| Seguridad (siempre que aplique) | 10 |

## 6. Condiciones de parada (obligatorias)

El agente DEBE detenerse y abrir una **solicitud de decisión** (`docs/templates/DECISION_REQUEST.md`, como issue o como archivo en `docs/decisions/pending/`) cuando:

1. Dos documentos o un documento y un contrato se contradicen.
2. Falta información para implementar un comportamiento observable.
3. La implementación correcta requiere cambiar un contrato público (OpenAPI, `.proto`, vectores, escenarios), un permiso, una política de retención, un parámetro de seguridad o un algoritmo criptográfico.
4. Se necesita una dependencia fuera de la lista aprobada.
5. Un requisito NFR no puede cumplirse con el diseño actual (tras medir).
6. Una prueba de contrato o un vector parece incorrecto.
7. Se tocaría código de otra tarea en curso.

Mientras espera, puede avanzar en partes de la tarea no afectadas o tomar otra tarea independiente, registrándolo en `PROGRESS.md`.

## 7. Qué puede decidir el agente sin preguntar
- Estructura interna de módulos y nombres de elementos privados dentro de un crate.
- Estructura de las pruebas y datos sintéticos.
- Refactorizaciones internas que no cambien comportamiento observable ni crucen reglas de dependencia.
- Elección entre alternativas equivalentes de la misma dependencia aprobada (p. ej. qué API concreta de `tokio` usar).
- Mensajes de error legibles (`message`/`title`) dentro de las reglas de `09` §7.

## 8. Evidencia y reporte
- Nunca afirmar que algo se ejecutó o pasó sin haberlo ejecutado. Si no se pudo ejecutar (falta de entorno, p. ej. iOS sin macOS), se dice explícitamente y la tarea queda **no cerrada** o se cierra solo la parte verificable.
- El informe incluye: tarea, requisitos cubiertos, archivos tocados, comandos ejecutados con su resultado resumido, pruebas nuevas, desviaciones, riesgos, trabajo pendiente.
- Las métricas medidas (benchmarks, plazos de revocación) se reportan con el entorno en que se midieron.

## 9. Anti-alucinación
- Verificar la API de cada crate/paquete **en la versión fijada** antes de usarla (docs.rs, pub.dev, código fuente en el registro local). Si una función no existe, no inventarla: buscar la alternativa real o parar (§6, condición 4).
- Los nombres de campos, rutas, códigos y mensajes salen de `contracts/`; no se reescriben de memoria.
- No suponer comportamiento de plataforma móvil: comprobar documentación oficial vigente y probar en dispositivo.
- Ante una duda sobre un número, buscarlo en `17`; si no está, parar (§6, condición 2).

## 10. Seguridad para agentes
- No escribir secretos reales en ningún archivo; usar valores de desarrollo de `deploy/.env.example`.
- No desactivar checks de CI, no bajar umbrales, no usar `--no-verify`, no saltar hooks.
- No ampliar permisos, scopes ni capacidades "para probar".
- No registrar coordenadas ni secretos, tampoco en pruebas (salvo los valores sintéticos de los vectores, que el verificador de logs conoce).
- No ejecutar comandos destructivos fuera del entorno de pruebas.

## 11. Varios agentes en paralelo
- Propiedad por área: una tarea en curso "posee" los crates/paquetes que declara en `PROGRESS.md`.
- Los cambios de contratos se serializan: solo un PR que toque `contracts/` abierto a la vez.
- Rebase frecuente sobre `main`; si un PR ajeno rompe el propio, resolver en la rama propia, nunca forzar en `main`.
- Las tareas marcadas "paralelizable" en el roadmap pueden ejecutarse a la vez si no comparten crates.

## 12. Prompt de arranque recomendado

Para iniciar una sesión, el propietario puede usar (ILUSTRATIVO):

```text
Trabajas en el repositorio GeoStream, gobernado por su documentación.
1. Lee AGENTS.md, docs/00_INDEX.md, docs/PROGRESS.md y docs/20_AGENT_PROTOCOL.md.
2. Toma la siguiente tarea pendiente de docs/21_ROADMAP.md según las reglas de selección.
3. Lee todos los documentos que la tarea indica y los requisitos que cita.
4. Sigue el ciclo de 20 §4. Si encuentras una condición de 20 §6, detente y crea la solicitud de decisión.
5. No declares la tarea terminada sin la Definition of Done de docs/19_ENGINEERING_STANDARDS.md §1
   y el informe con comandos y resultados reales.
```
