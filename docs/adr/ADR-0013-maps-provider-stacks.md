# ADR-0013: Mapas como adaptadores agrupados en stacks; MapLibre primero

- **Estado:** Aceptado (baseline v0.2.0)
- **Fecha:** 2026-10-11
- **Responsable:** propietario del proyecto
- **Requisitos afectados:** FR-MAP-*, NFR-RES-001, NFR-COST-001

## Contexto

Los términos de Google Maps Platform prohíben usar contenido de sus APIs de geocodificación, rutas, lugares, etc. con mapas que no sean de Google, por lo que los adaptadores no pueden combinarse libremente. El núcleo debe funcionar sin proveedor comercial.

## Opciones consideradas

1. Google Maps como primer adaptador.
2. MapLibre como primer adaptador, Google como segundo, ambos dentro de `ProviderStack` con reglas de compatibilidad.
3. Sin adaptadores (solo datos).

## Decisión

Opción 2. Paquete de interfaces `geostream_maps`; `ProviderStack` rechaza combinaciones prohibidas; atribución obligatoria; interpolación solo visual. Los términos se revalidan en cada PR de adaptador.

## Consecuencias

+ Núcleo sin costo de proveedor; cumplimiento de licencias por construcción; privacidad mejor con teselas propias.
- MapLibre requiere elegir y posiblemente alojar teselas.

## Seguridad, privacidad y operación

Las peticiones de teselas revelan la zona observada al proveedor; documentado en `13` §7.

## Referencias

`13_MAPS_PROVIDERS.md`; https://cloud.google.com/maps-platform/terms/maps-service-terms
