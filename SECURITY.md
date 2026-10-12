# Política de seguridad

## Reportar una vulnerabilidad
No abras un issue público. Envía un informe privado al propietario del proyecto (canal a definir antes del primer release público; mientras tanto, usa el mecanismo de *private vulnerability reporting* del repositorio en GitHub cuando esté habilitado).

Incluye: componente y versión, pasos para reproducir, impacto esperado y, si es posible, una propuesta de mitigación. No incluyas datos de ubicación reales de personas.

## Compromisos (a partir del primer release público)
- Acuse de recibo en ≤ 3 días hábiles.
- Evaluación inicial y severidad en ≤ 10 días hábiles.
- Corrección coordinada y aviso en `CHANGELOG.md` (sección `Security`).

## Alcance
Servidor, contratos, SDK Flutter y adaptadores de este repositorio. El modelo de amenazas está en `docs/10_SECURITY_PRIVACY.md`.
