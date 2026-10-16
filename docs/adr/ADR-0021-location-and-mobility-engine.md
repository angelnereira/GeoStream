# ADR-0021: GeoStream como motor de ubicación y movilidad (alcance y no objetivos)

- **Estado:** Aceptado
- **Fecha:** 2026-10-15
- **Responsable:** propietario del proyecto
- **Requisitos afectados:** FR-TRK-*, FR-SMP-*, FR-POOL-*, FR-MOB-*, FR-HIS-*, FR-ANA-*, FR-LRN-*, FR-EDGE-*, FR-CORE-*, FR-BND-*
- **Sustituye a:** Enmienda la línea base de `01_VISION_SCOPE.md`

## Contexto

La línea base inicial (`v0.2.0`) concebía GeoStream únicamente como capa de transporte y autorización de ubicación efímera, dejando fuera del alcance cualquier filtrado, historial, analítica, herramientas de movilidad (pools, búsqueda de cercanos, ETA, asignación), dispositivos sin internet y bindings fuera de Flutter (NG-4, NG-5 y NG-6).

Por las decisiones del propietario **D-01, D-02, D-03, D-07, D-08, D-09, D-10 y D-12** (`GS-ENG-A02` §3), GeoStream pasa a ser la tecnología núcleo sobre la que se construyen aplicaciones de movilidad tipo Uber/InDrive, reparto, flotas y rastreo de activos, operando tanto en el servidor (Linux) como en el dispositivo del usuario final.

## Opciones consideradas

1. Mantener GeoStream restringido a transporte efímero (obliga a cada integrador a reinventar filtros de Kalman, muestreo adaptativo, pools H3, ETA, asignación, historial y relevo BLE).
2. Expandir GeoStream a una plataforma completa de negocio vertical (pedidos, tarifas, pagos, sanciones antifraude), lo que lo volvería rígido e incompatible con múltiples verticales.
3. Expandir GeoStream a un **motor de ubicación y movilidad** modular con dos mitades (servidor y dispositivo) que comparten núcleo Rust, aportando primitivas y herramientas matemáticas/espaciales mientras la lógica de negocio (pedidos, tarifas, pagos, reglas de despacho y sanciones antifraude) permanece en el integrador.

## Decisión

Adoptar la Opción 3 (decisiones D-01, D-02, D-10 y D-12):
- GeoStream es el motor de ubicación y movilidad en tiempo real e histórico, compuesto por dos mitades (servidor Linux y núcleo en dispositivo) que hablan entre sí y comparten código Rust (`gs-domain`, `gs-protocol`, `gs-geo`, `gs-track`, `gs-edge`).
- Se reformulan los no-objetivos en `01_VISION_SCOPE.md` §5:
  - **NG-4:** GeoStream no implementa lógica de negocio de movilidad (pedidos, tarifas, pagos, políticas de despacho, incentivos ni sanciones antifraude), pero sí provee las primitivas y herramientas de proximidad (`POOL`, `nearby`, `rings`, `density`), ETA, asignación por lotes como cálculo, rutas vía proveedor y map-matching.
  - **NG-5:** El historial y la analítica son capacidades centrales, estrictamente opt-in por política del tenant, modo de sesión (`history_mode`) y consentimiento con propósito del subject (`consent_purposes`).
  - **NG-6:** La instalación mínima sigue siendo «un binario + PostgreSQL»; H3, grafo vial, proveedores, historial y edge se activan por política o *feature* (principio P-E7).

## Consecuencias

+ Convierte a GeoStream en una dependencia completa y reutilizable para cualquier vertical de movilidad o localización.
+ Mantiene la neutralidad de negocio: el integrador conserva el control de sus reglas comerciales y antifraude.
- Incrementa el alcance técnico, que se gestiona mediante la pista de fases E0–E9 y criterios de GA escalonados (`21_ROADMAP.md`).

## Seguridad, privacidad y operación

Se preservan íntegros los principios de privacidad por defecto (compartición apagada por defecto, redacción estricta en logs y métricas, aislamiento por `tenant_id`) y se amplía el modelo de amenazas con T-23..T-31 en `10_SECURITY_PRIVACY.md`.

## Plan de migración

Aditivo sobre lo implementado en F0–F4. Los contratos v1 incorporan las extensiones en E0 antes de la congelación en T-F5-13 (M2).

## Evidencia y referencias

- Decisiones D-01, D-02, D-10, D-12 en `docs/proposals/GeoStream_Addendum_A02_Motor_Ubicacion_Movilidad.md` §3.
- `docs/01_VISION_SCOPE.md`, `docs/04_ARCHITECTURE.md`.
