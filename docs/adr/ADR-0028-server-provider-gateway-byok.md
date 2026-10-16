# ADR-0028: Provider Gateway en el servidor con BYOK y manifiestos (adopción de A-01 §5)

- **Estado:** Aceptado
- **Fecha:** 2026-10-15
- **Responsable:** propietario del proyecto
- **Requisitos afectados:** FR-MAP-001..006, FR-MOB-001, FR-MOB-003, NFR-RES-001, NFR-COST-001
- **Sustituye a:** Enmienda ADR-0013 y `13_MAPS_PROVIDERS.md` §8

## Contexto

En `v0.2.0` (`13_MAPS_PROVIDERS.md` §8) el servidor no invocaba proveedores externos. Sin embargo, para ordenar candidatos de un pool por ETA real en el servidor (`nearby` con `rank=eta`), calcular matrices (`POST /v1/tools/eta`) o ejecutar map-matching por lotes (`gs-analytics`) sin exponer claves de servidor en las apps móviles, el servidor necesita conectarse opcionalmente a proveedores (OSRM, Valhalla, Google Routes/Roads) manteniendo la regla `NFR-COST-001` (funcionar sin proveedores comerciales por defecto) y respetando estrictamente los términos de licencia de cada proveedor (Addendum A-01).

## Opciones consideradas

1. Prohibir proveedores en el servidor (obliga a calcular ETA solo con línea recta o a exponer secretos de rutas en clientes).
2. Usar una clave global del operador de GeoStream para todos los tenants (convierte al operador en "Customer" único ante Google/proveedores y mezcla cuotas).
3. **Adoptar el modelo de puertos con manifiesto declarativo, `ContentPolicy` de procedencia y el Provider Gateway del lado servidor (`gs-providers`) con credenciales aportadas por cada tenant (BYOK, *Bring Your Own Key*).**

## Decisión

Adoptar la Opción 3 (decisiones D-02 y D-08, y Addendum A-01 §4–§6), enmendando ADR-0013 y `13_MAPS_PROVIDERS.md` §8:
- Se adopta el marco de **puertos, adaptadores, manifiestos declarativos y `ContentPolicy`** de A-01 tanto en cliente como en servidor.
- Se crea el crate `gs-providers` en el servidor con los puertos `RouteMatrixProvider`, `RouteProvider`, `MapMatchingProvider` y `GeocodingProvider`.
- **Regla BYOK:** el servidor solo invoca a un proveedor externo para los tenants que lo hayan configurado explícitamente (`GET/PUT/DELETE /v1/providers/{port}`, scope `providers:manage`) con sus propias credenciales, las cuales se almacenan cifradas en reposo con AES-256-GCM (`10` §5.6).
- **Orden de implementación de adaptadores:** primero autoalojables/abiertos (OSRM, Valhalla) y después Google Routes/Roads con las restricciones de `ContentPolicy` verificadas en A-01 §3.1.
- **Aislamiento y degradación (`NFR-RES-001`):** un fallo del proveedor nunca afecta al stream ni a la aceptación; en ETA cae automáticamente al nivel `LEARNED` o `ESTIMATED` salvo que la petición exija `source_preference=provider` (`503 PROVIDER_UNAVAILABLE`).

## Consecuencias

+ Mantiene `NFR-COST-001`: una instalación sin proveedores configurados opera al 100 % con los estimadores locales y la capa aprendida.
+ Cumplimiento contractual por construcción mediante manifiestos y `ContentPolicy` (ningún dato de proveedor comercial alimenta la capa aprendida propia).

## Seguridad, privacidad y operación

Se añade la amenaza T-29 en `10_SECURITY_PRIVACY.md`: hacia el proveedor solo salen las coordenadas estrictamente necesarias para el cálculo, nunca IDs de subject, dispositivo ni sesión, y las credenciales BYOK nunca aparecen en logs ni respuestas.

## Plan de migración

Se especifica en `13_MAPS_PROVIDERS.md` y `23_MOBILITY.md` en E0 y se implementa en E5 (`T-E5-01..04`) y E7 (`T-E7-03`).

## Evidencia y referencias

- Decisiones D-02 y D-08 en `docs/proposals/GeoStream_Addendum_A02_Motor_Ubicacion_Movilidad.md` §3 y §10.1.
- `docs/proposals/GeoStream_Addendum_A01_Integracion_Proveedores_Externos.md` §3–§8.
