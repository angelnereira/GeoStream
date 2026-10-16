# ADR-0024: Historial opt-in por política, modo de sesión y consentimiento con propósito; analítica con k-anonimato

- **Estado:** Aceptado
- **Fecha:** 2026-10-15
- **Responsable:** propietario del proyecto
- **Requisitos afectados:** FR-PRV-001, FR-HIS-001..007, FR-ANA-001..004, NFR-PRIV-001, NFR-PRIV-003, NFR-PERF-009
- **Sustituye a:** ADR-0005

## Contexto

ADR-0005 establecía que GeoStream no guardaba historial en v1 y difería cualquier persistencia de trayectorias a una pista futura F13. Por la decisión **D-07** del propietario (`GS-ENG-A02` §3), guardar historial, trazas, viajes y mapas de calor para evaluar el comportamiento de movilidad pasa a ser una capacidad central del motor dentro de la pista E (fases E6 y E7), manteniendo el carácter efímero por defecto cuando no se habilita.

## Opciones consideradas

1. Activar historial globalmente para todas las sesiones (viola la minimización de datos y el consentimiento granular del subject).
2. Activar historial solo con un flag de sesión fijado por la aplicación, sin consentimiento del subject.
3. **Triple llave explícita (política + sesión + consentimiento con propósito del subject) y k-anonimato obligatorio en agregados.**

## Decisión

Adoptar la Opción 3 (decisión D-07), sustituyendo a ADR-0005:
1. **Efímero por defecto se mantiene:** toda sesión tiene `history_mode = OFF` por defecto y todo tenant tiene `history_allowed = false` y `analytics_allowed = false` por defecto. Sin esas llaves activas, no se escribe ninguna coordenada en PostgreSQL (`NFR-PRIV-001`).
2. **Triple condición para escribir historial:**
   - `tenant.history_allowed == true`,
   - `session.history_mode ∈ {TRACE, TRACE_AND_ANALYTICS}`, y
   - el propio subject otorgó `HISTORY ∈ member.consent_purposes` desde su dispositivo al activar la compartición (`INV-SHR-5`).
3. **Condición adicional para analítica:** `tenant.analytics_allowed == true ∧ session.history_mode == TRACE_AND_ANALYTICS ∧ ANALYTICS ∈ member.consent_purposes`.
4. **Almacenamiento y camino de escritura:** tabla `location_points` particionada por rango diario (`captured_at`) en PostgreSQL sin requerir PostGIS ni `pg_partman`, escrita por lotes de forma asíncrona por un canal acotado (`GS_HISTORY_QUEUE_CAPACITY`) que nunca bloquea la aceptación ni el fan-out (`P-E4`).
5. **k-anonimato (`NFR-PRIV-003`):** todo agregado expuesto (mapas de calor, matrices OD, velocidades, lugares aprendidos) suprime cualquier celda o lugar con menos de `analytics_k_anonymity` subjects distintos.

## Consecuencias

+ Habilita trazas, resumen de viajes (`trips`), mapas de calor y aprendizaje de red vial sin comprometer el camino caliente.
+ Garantiza control estricto de privacidad: el subject decide explícitamente si consiente `HISTORY` y/o `ANALYTICS`, y su retiro detiene las escrituras de inmediato.
- Exige gestión automática de particiones diarias y supresión extendida en `DELETE /v1/subjects/{id}` (`11_DATA_STORAGE.md` §2, §5).

## Seguridad, privacidad y operación

Se añaden las amenazas T-13 (enmendada), T-23 y T-24 en `10_SECURITY_PRIVACY.md`. Antes del release de producción de E6 se requiere revisión de DPIA y textos de consentimiento (tarea T-E6-11).

## Plan de migración

Los campos `history_mode` y `consent_purposes` se incorporan al contrato en E0 (T-E0-06) y las tablas particionadas se crean en la migración de E6 (T-E6-02).

## Evidencia y referencias

- Decisión D-07 en `docs/proposals/GeoStream_Addendum_A02_Motor_Ubicacion_Movilidad.md` §3 y §11.
- `docs/24_HISTORY_ANALYTICS.md`, `docs/10_SECURITY_PRIVACY.md`, `docs/11_DATA_STORAGE.md`.
