# ADR-0030: Predicción visual en el cliente

- **Estado:** Aceptado
- **Fecha:** 2026-10-15
- **Responsable:** propietario del proyecto
- **Requisitos afectados:** FR-MAP-005 (enmendado), FR-SMP-005
- **Sustituye a:** Enmienda la prohibición de extrapolar de `FR-MAP-005` y `13_MAPS_PROVIDERS.md` §6

## Contexto

El requisito original `FR-MAP-005` permitía interpolar visualmente entre dos muestras ya recibidas (lo que introduce retardo visual de un intervalo de muestreo), pero prohibía extrapolar hacia adelante. Con la decisión **D-06** del propietario (`GS-ENG-A02` §3) y el muestreo adaptativo que espacia las publicaciones cuando la trayectoria es rectilínea (§8.3), la UI del usuario final se beneficia de una predicción visual suave (*dead reckoning*) a 60 fps siempre que esté acotada, etiquetada y nunca se confunda con una medición.

## Opciones consideradas

1. Prohibir toda extrapolación (el marcador se queda quieto hasta el siguiente fix o va con un intervalo de retraso).
2. Extrapolar linealmente sin límite de tiempo ni decaimiento (el marcador atraviesa edificios o se pasa de los semáforos durante pérdidas de señal).
3. **Permitir predicción visual acotada en el núcleo del cliente (`MemberView.renderPosition(nowMs)`), con decaimiento exponencial de velocidad, proyección opcional sobre polilínea de ruta, radio de incertidumbre creciente, suavizado críticamente amortiguado al reconciliar y etiqueta obligatoria `LocationKind.PREDICTED`.**

## Decisión

Adoptar la Opción 3 (decisión D-06), enmendando `FR-MAP-005` y `13_MAPS_PROVIDERS.md` §6:
- `MemberView.renderPosition(now_ms)` en `gs-client-core` devuelve `{position, kind, confidence_radius_m}` con `kind ∈ {MEASURED, FILTERED, PREDICTED}`.
- **Modelo cinemático con decaimiento:** `dist(Δt) = v · τ · (1 − e^(−Δt / τ))` con `τ = predict_speed_decay_s` y horizonte duro `Δt ≤ predict_horizon_ms` (inicial 3000 ms, máximo 10 000 ms).
- **Sobre ruta:** si el integrador suministra una polilínea activa a la vista, el avance se proyecta a lo largo de la polilínea (`gs-geo` *along-track*) sin sobrepasar su vértice final.
- **Condiciones de desactivación automática:** entidad en estado quieto (`STOPPED`/`STILL`), `plausibility < predict_min_plausibility`, `freshness == stale` o `speed_mps < predict_min_speed_mps`.
- **Reconciliación suave:** cuando llega una actualización real del servidor, el marcador converge en `predict_blend_ms` mediante amortiguamiento crítico, sin saltos bruscos ni retrocesos.
- **Regla inviolable:** una posición `PREDICTED` es exclusivamente local para renderizado en el cliente; nunca se publica al servidor, nunca se guarda en historial y nunca se escribe en logs.

## Consecuencias

+ Experiencia visual fluida tipo Uber en todos los SDKs (Flutter, Kotlin, Swift, Web) con una única implementación matemática en `gs-client-core` verificada con los vectores `PRD`.
+ Transparencia total: el integrador recibe `kind = PREDICTED` y `confidence_radius_m` para reflejar la incertidumbre en la UI.

## Seguridad, privacidad y operación

No altera datos transmitidos ni almacenados; el servidor continúa rechazando cualquier muestra entrante que no sea `MEASURED` según las reglas de publicación del cliente.

## Plan de migración

Se documenta en `13_MAPS_PROVIDERS.md` §6 y `22_TRACKING_ENGINE.md` en E0 y se implementa en `gs-client-core` durante E3 (T-E3-04).

## Evidencia y referencias

- Decisión D-06 en `docs/proposals/GeoStream_Addendum_A02_Motor_Ubicacion_Movilidad.md` §3 y §8.6.
- `docs/13_MAPS_PROVIDERS.md` §6, `docs/22_TRACKING_ENGINE.md`.
