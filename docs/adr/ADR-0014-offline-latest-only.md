# ADR-0014: Política offline del SDK: solo la última muestra

- **Estado:** Aceptado (baseline v0.2.0)
- **Fecha:** 2026-10-11
- **Responsable:** propietario del proyecto
- **Requisitos afectados:** FR-SDK-008 (sustituye el FR-033 de la propuesta v0.1.0)

## Contexto

En 0.1.0 la cola offline acumulada era obligatoria, pero con semántica latest-state y sin historial, subir puntos viejos al reconectar no aporta nada: el servidor los descarta por antiguos.

## Opciones consideradas

1. Cola acumulada cifrada siempre.
2. Solo la última muestra por sesión, en memoria, descartada si supera la antigüedad máxima.
3. Sin cola.

## Decisión

Opción 2 (`latestOnly`) en v1. La cola acumulada (`buffered`, cifrada) solo existirá con historial (F13). Detener la compartición sin conexión se aplica localmente al instante y se reintenta en el servidor de forma persistente.

## Consecuencias

+ SDK mucho más simple; ninguna coordenada en disco del dispositivo.
- Sin recuperación de recorridos durante cortes de red.

## Seguridad, privacidad y operación

Privacidad: no se persisten ubicaciones en el dispositivo en v1.

## Referencias

`12_FLUTTER_SDK.md` §13
