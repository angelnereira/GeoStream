# ADR-0019: Licencia del proyecto

- **Estado:** Propuesto — requiere decisión del propietario
- **Fecha:** 2026-10-11
- **Responsable:** propietario del proyecto
- **Requisitos afectados:** NFR-SEC-006 (compatibilidad de licencias), publicación de SDK

## Contexto

GeoStream incluye un servidor y SDKs que terceros integrarán. La licencia condiciona la adopción, las dependencias permitidas y el modelo de negocio (por ejemplo, servicio gestionado).

## Opciones consideradas

1. Apache-2.0 para todo (permisiva, con concesión de patentes).
2. MIT OR Apache-2.0 (doble licencia habitual en Rust).
3. Servidor con licencia restrictiva o propietaria y SDK/contratos permisivos (open core).
4. Todo propietario.

## Decisión

Pendiente. Recomendación técnica: SDK y contratos con licencia permisiva (Apache-2.0 o MIT OR Apache-2.0) para facilitar la integración; la licencia del servidor la decide el propietario según el modelo de negocio. Hasta decidir: no añadir archivo `LICENSE`, no publicar paquetes, y mantener la lista de licencias de dependencias de `19` §6.1 (compatible con cualquiera de las opciones).

## Consecuencias

+ Decisión explícita antes de publicar.
- Bloquea la publicación en registros (no la implementación).

## Seguridad, privacidad y operación

Verificar compatibilidad con las licencias de dependencias en `cargo deny`.

## Referencias

`19_ENGINEERING_STANDARDS.md` §6.1
