# ADR-0016: Geocercas evaluadas en proceso con R-tree; Tile38 y H3 diferidos

- **Estado:** Sustituido parcialmente por [ADR-0026](ADR-0026-h3-and-rtree-spatial-indexing.md) (mantiene R-tree en proceso para polígonos de geocercas)
- **Fecha:** 2026-10-11
- **Responsable:** propietario del proyecto
- **Requisitos afectados:** FR-GEO-001..004

## Contexto

La propuesta 0.1.0 sugería H3 y Tile38. Con los límites de política previstos (miles de geocercas por tenant), un índice en memoria es suficiente y evita otro servicio que operar.

## Opciones consideradas

1. Tile38 como servicio.
2. H3 como índice.
3. R-tree en memoria (`rstar`) + geometría exacta (`geo`) en el rol `stream`, con histéresis.

## Decisión

Opción 3. Tile38/H3 solo si mediciones muestran memoria o latencia fuera de NFR con los límites de política, mediante ADR nuevo. La evaluación nunca bloquea la distribución.

## Consecuencias

+ Sin dependencias operativas nuevas; latencia mínima.
- Cada instancia mantiene su índice; memoria proporcional al número de geocercas.

## Seguridad, privacidad y operación

Eventos sin coordenadas en webhooks.

## Referencias

`14_GEOFENCING.md`
