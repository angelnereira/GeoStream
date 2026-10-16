# ADR-0025: Cola offline `buffered` cifrada para sesiones con historial

- **Estado:** Aceptado
- **Fecha:** 2026-10-15
- **Responsable:** propietario del proyecto
- **Requisitos afectados:** FR-SDK-008, FR-SMP-006, FR-HIS-002
- **Sustituye a:** ADR-0014

## Contexto

ADR-0014 fijaba únicamente la política `latestOnly` (conservar en memoria solo la última muestra durante cortes de red), porque sin historial en el servidor los puntos antiguos carecían de utilidad. Con la adopción del historial opt-in (ADR-0024, decisión **D-07**) y los casos de seguimiento de rutas y flotas en zonas con cobertura intermitente, perder el recorrido durante un corte de red impide reconstruir el viaje real y el odómetro completo.

## Opciones consideradas

1. Mantener solo `latestOnly` en todos los modos (pierde trazas durante cortes de red en sesiones con historial).
2. Almacenar muestras offline en texto plano en el almacenamiento local de la app (riesgo de exposición de historial de ubicación ante acceso físico o backup del dispositivo).
3. **Política dual según la sesión (`latestOnly` sin historial, `buffered` cifrada en reposo con simplificación online y ruta de backfill dedicada cuando hay historial).**

## Decisión

Adoptar la Opción 3, sustituyendo a ADR-0014:
- **`latestOnly`** se mantiene para sesiones con `history_mode == OFF`: conserva solo la última muestra en memoria y la descarta si supera `max_event_age_ms`.
- **`buffered`** se activa cuando la sesión tiene historial (`history_mode ≠ OFF`) y el subject consintió `HISTORY`:
  - Las muestras se simplifican en línea por ventana (`offline_simplify_tolerance_m`, `gs-geo`) antes de persistirse.
  - La cola se cifra en reposo con **AES-256-GCM**, cuya clave se custodia en el almacén seguro de la plataforma (`SecureStore`: Android Keystore / iOS Keychain).
  - Está acotada por `offline_buffer_max_samples` y `offline_buffer_max_age_s`.
  - Al recuperar conectividad, las muestras recientes (`≤ max_event_age_ms`) se envían por el stream vivo, y las más antiguas (`≤ history_backfill_max_age_s`) se suben por lotes a `POST /v1/ingest/history`, que escribe únicamente en el historial sin alterar el snapshot vivo ni el fan-out.

## Consecuencias

+ Recupera recorridos completos tras desconexiones en túneles o zonas rurales sin contaminar el stream en tiempo real con muestras obsoletas.
+ Protege las coordenadas en disco mediante cifrado autenticado respaldado por hardware/SO.
- Requiere implementar el puerto `SecureStore` en los anfitriones móviles y el endpoint `POST /v1/ingest/history` en E6.

## Seguridad, privacidad y operación

Si el usuario detiene la compartición o retira el consentimiento `HISTORY` estando sin conexión, la cola `buffered` de esa sesión se purga y destruye criptográficamente de inmediato en el dispositivo.

## Plan de migración

`gs-client-core` implementa `latestOnly` en E3 (T-E3-05) y añade `buffered` + backfill en E6 (T-E6-04).

## Evidencia y referencias

- Decisión D-07 en `docs/proposals/GeoStream_Addendum_A02_Motor_Ubicacion_Movilidad.md` §8.5 y §11.2.
- `docs/12_FLUTTER_SDK.md` §13, `docs/22_TRACKING_ENGINE.md`, `docs/24_HISTORY_ANALYTICS.md`.
