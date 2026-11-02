// Relevo de seguimiento para el cliente final de una entrega (docs/15 §3.2, DR-20261102-01 opción 1).
//
// El navegador del cliente NO puede leer el snapshot de GeoStream (solo una aplicación con
// `locations:read`). Este servicio es el del integrador: guarda la API key, lee el snapshot y devuelve
// SOLO lo que el cliente necesita. Sin dependencias: Node >= 20.
//
//   GEOSTREAM_URL=https://geostream.gravitalcloud.com GEOSTREAM_API_KEY=gsk_... node relay.mjs
//
// Cada pedido tiene un `tracking token` OPACO y aleatorio, propio del integrador: nunca el session_id
// de GeoStream. Aquí se registra con `register()`; en producción vive en tu base de datos.
import { createServer } from 'node:http';
import { randomBytes } from 'node:crypto';

const BASE = (process.env.GEOSTREAM_URL ?? '').replace(/\/$/, '');
const API_KEY = process.env.GEOSTREAM_API_KEY ?? '';
const CACHE_MS = Number(process.env.CACHE_MS ?? 2000);
const TOKEN_RE = /^[A-Za-z0-9_-]{32,64}$/;

/** token opaco -> { sessionId, driverSubject, expiresAt } */
const orders = new Map();
/** sessionId -> { at, value } : una lectura a GeoStream por sesión cada CACHE_MS, sin importar cuántos miran. */
const cache = new Map();
const inflight = new Map();

export function register(sessionId, driverSubject, ttlMs = 2 * 60 * 60 * 1000) {
  const token = randomBytes(24).toString('base64url');
  orders.set(token, { sessionId, driverSubject, expiresAt: Date.now() + ttlMs });
  return token;
}

async function readSnapshot(sessionId) {
  const hit = cache.get(sessionId);
  if (hit && Date.now() - hit.at < CACHE_MS) return hit.value;
  if (inflight.has(sessionId)) return inflight.get(sessionId);
  const p = (async () => {
    const res = await fetch(`${BASE}/v1/sessions/${sessionId}/snapshot`, {
      headers: { Authorization: `Bearer ${API_KEY}` },
      signal: AbortSignal.timeout(8000),
    });
    // 404 sesión inexistente; 409 cerrada o expirada. El resto es un fallo nuestro.
    const value = { status: res.status, body: res.ok ? await res.json() : null };
    cache.set(sessionId, { at: Date.now(), value });
    return value;
  })().finally(() => inflight.delete(sessionId));
  inflight.set(sessionId, p);
  return p;
}

function send(res, status, body) {
  res.writeHead(status, { 'Content-Type': 'application/json', 'Cache-Control': 'no-store' });
  res.end(JSON.stringify(body));
}

export const server = createServer(async (req, res) => {
  const m = /^\/track\/([^/]+)$/.exec(new URL(req.url, 'http://x').pathname);
  if (req.method !== 'GET' || !m || !TOKEN_RE.test(m[1])) return send(res, 404, { error: 'not_found' });
  const order = orders.get(m[1]);
  if (!order || order.expiresAt < Date.now()) return send(res, 404, { error: 'not_found' });

  let snap;
  try {
    snap = await readSnapshot(order.sessionId);
  } catch {
    return send(res, 502, { error: 'tracking_unavailable' });
  }
  if (snap.status === 404 || snap.status === 409) return send(res, 200, { state: 'finished' });
  if (snap.status !== 200) return send(res, 502, { error: 'tracking_unavailable' });

  const item = snap.body.items.find((i) => i.external_subject_id === order.driverSubject);
  if (!item) return send(res, 200, { state: 'waiting' }); // el repartidor aún no comparte
  const p = item.sample.position;
  // Solo lo necesario para dibujar el punto: sin member_id, sin precisión interna, sin otros miembros.
  send(res, 200, {
    state: 'live',
    lat: p.latitude,
    lon: p.longitude,
    heading: p.heading_deg ?? null,
    age_s: Math.round(item.age_ms / 1000),
  });
});

if (import.meta.url === `file://${process.argv[1]}`) {
  if (!BASE || !API_KEY) throw new Error('define GEOSTREAM_URL y GEOSTREAM_API_KEY');
  server.listen(Number(process.env.PORT ?? 8787), () => console.log('relevo escuchando'));
}
