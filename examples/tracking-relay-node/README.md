# Relevo de seguimiento (Node, sin dependencias)

Ejemplo ejecutable del patrón de `docs/15` §3.2 elegido en `DR-20261102-01`: la página de seguimiento del
integrador consulta **su propio** backend y este lee el snapshot de GeoStream con su API key.

- `GET /track/<token>` → `{state:"waiting"}`, `{state:"live",lat,lon,heading,age_s}` o `{state:"finished"}`.
- El `<token>` es opaco y propio del pedido; **no** es el `session_id` de GeoStream.
- Una sola lectura a GeoStream por sesión cada `CACHE_MS` (2 s), aunque miren cien clientes: el límite de
  la API key es compartido por todo el proyecto (`GS_REST_RATE_LIMIT_APP_RPM`).
- La API key necesita `locations:read` (y `sessions:read`). Nunca va al navegador.

Es un ejemplo: la tabla de pedidos (`orders`) debe vivir en tu base de datos, con caducidad, y la ruta debe
llevar tu propio límite de tasa por IP.
