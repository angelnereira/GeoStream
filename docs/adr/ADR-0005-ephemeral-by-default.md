# ADR-0005: Ubicación efímera por defecto

- **Estado:** Sustituido por [ADR-0024](ADR-0024-opt-in-history-and-k-anonymous-analytics.md) (mantiene ubicación efímera por defecto cuando el historial no está activo)
- **Fecha:** 2026-10-11
- **Responsable:** propietario del proyecto
- **Requisitos afectados:** NFR-PRIV-001, NFR-PRIV-002, FR-PRV-001

## Contexto

Las ubicaciones revelan domicilio, rutinas y relaciones. El caso base es compartir en tiempo real, no conservar trayectorias.

## Opciones consideradas

1. Historial siempre.
2. Sin historial por defecto; snapshot efímero con TTL; historial opt-in posterior.
3. Sin snapshot (solo en vivo).

## Decisión

Opción 2. Sin historial. Snapshot por miembro con `snapshot_ttl_seconds` (0 lo desactiva), borrado inmediato al dejar de compartir, revocar, salir o cerrar. Redis/Valkey sin persistencia en disco. Historial solo como pista F13 con ADR de base legal.

## Consecuencias

+ Menor exposición y costo; cumplimiento más sencillo.
- Sin analítica histórica ni reconstrucción de recorridos en v1.

## Seguridad, privacidad y operación

Lo ya entregado a otros participantes no puede retirarse; se documenta (`10` §6.1).

## Referencias

`10_SECURITY_PRIVACY.md` §6–§7
