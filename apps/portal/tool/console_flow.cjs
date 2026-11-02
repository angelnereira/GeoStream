// Recorrido de la consola en un navegador real (FR-CON-001, FR-CON-013, FR-CON-004, FR-CON-012, ADR-0037).
//
// Registro con contraseña, recarga (la sesión sobrevive), cierre de sesión, entrada en dos pasos
// (contraseña y código), organización, proyecto y API key; comprueba que la key autentica contra la API
// real y que el secreto no queda en pantalla ni en el almacenamiento del navegador.
//
// No forma parte de CI (no hay un entorno con Chrome y servidor); se ejecuta a mano:
//   1. `geostream serve --roles api` con GS_CONSOLE_OTP_HMAC_KEY, GS_CONSOLE_EMAIL_PROVIDER=file,
//      GS_CONSOLE_EMAIL_DIR=<dir>/mail y GS_CONSOLE_PASSWORD_ITERATIONS=100000 (mínimo, para no esperar).
//   2. El portal compilado (`flutter build web --release --no-web-resources-cdn`) detrás de nginx con la
//      plantilla `nginx.conf.template`, apuntando a ese servidor.
//   3. `npm i puppeteer-core` y: PORTAL_URL=http://127.0.0.1:18088 MAIL_DIR=<dir>/mail node console_flow.cjs <dir>
// Flutter Web expone los campos de texto como <input aria-label=…> y el resto por el árbol de accesibilidad,
// que este script activa; los clics son del ratón real porque Flutter ignora los eventos sintéticos.
const puppeteer = require('puppeteer-core');
const fs = require('fs');
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const OUT = process.argv[2] || '.';
const BASE = process.env.PORTAL_URL || 'http://127.0.0.1:18088';
const MAIL = process.env.MAIL_DIR || OUT + '/mail';
const EMAIL = `ui.${Date.now()}@example.test`;
const PASSWORD = 'una frase larga y segura 2026';

(async () => {
  const b = await puppeteer.launch({ executablePath: '/usr/bin/google-chrome', headless: 'new', args: ['--no-sandbox', '--enable-unsafe-swiftshader'] });
  const p = await b.newPage();
  await b.defaultBrowserContext().overridePermissions(BASE, ['clipboard-read', 'clipboard-write']);
  await p.setViewport({ width: 1100, height: 900 });
  const issues = [];
  let step = 'inicio';
  let createdSecret = '';
  p.on('response', async (r) => { if (r.request().method() === 'POST' && /\/projects\/app_[0-9a-f]{32}\/keys$/.test(r.url()) && r.status() === 201) { try { createdSecret = (await r.json()).secret; } catch {} } });
  p.on('console', (m) => { if (m.type() === 'error') issues.push(m.text().slice(0, 160)); });
  p.on('pageerror', (e) => issues.push('pageerror en «' + step + '»'));

  const labels = () => p.evaluate(() => [...document.querySelectorAll('flt-semantics')].map((e) => (e.getAttribute('aria-label') || e.innerText || '').trim()).filter(Boolean));
  const find = (text, nth = 0) => p.evaluate((t, n) => {
    const text = (e) => ((e.getAttribute('aria-label') || '') + ' ' + (e.innerText || '')).trim();
    const all = [...document.querySelectorAll('flt-semantics')].filter((e) => text(e).includes(t));
    // The deepest matches only: ancestors repeat the text of everything they contain.
    const els = all.filter((e) => !all.some((o) => o !== e && e.contains(o)));
    const e = els[n < 0 ? els.length + n : n];
    if (!e) return null;
    const r = e.getBoundingClientRect();
    return { x: r.x + r.width / 2, y: r.y + r.height / 2, w: r.width, h: r.height };
  }, text, nth);
  const click = async (text, nth = 0) => {
    for (let i = 0; i < 20; i++) {
      const r = await find(text, nth);
      if (r && r.w > 0) { await p.mouse.click(r.x, r.y); await sleep(500); return; }
      await sleep(300);
    }
    throw new Error('no se encontró para pulsar: ' + text + ' | ' + JSON.stringify((await labels()).map((l) => l.slice(0, 50)).slice(0, 25)));
  };
  const type = async (label, text) => {
    for (let i = 0; i < 20; i++) {
      const handle = await p.$(`input[aria-label="${label}"], textarea[aria-label="${label}"]`);
      const box = handle && (await handle.boundingBox());
      if (box && box.width > 0) {
        await p.mouse.click(box.x + box.width / 2, box.y + box.height / 2);
        await sleep(250);
        await p.keyboard.type(text, { delay: 5 });
        await sleep(200);
        return;
      }
      await sleep(300);
    }
    const dom = await p.evaluate(() => [...document.querySelectorAll('input,textarea')].map((e) => e.getAttribute("aria-label")));
    throw new Error('no se encontró el campo: ' + label + ' | ' + JSON.stringify(dom));
  };
  const has = async (text) => (await labels()).some((l) => l.includes(text));
  const waitFor = async (text, ms = 15000) => { const t = Date.now(); while (Date.now() - t < ms) { if (await has(text)) return; await sleep(300); } throw new Error('no apareció: ' + text + ' | ' + JSON.stringify((await labels()).map((l) => l.slice(0, 50)).slice(0, 25))); };
  const shot = (name) => p.screenshot({ path: `${OUT}/ui-${name}.png` });
  const latestCode = () => {
    const f = fs.readdirSync(MAIL).map((n) => ({ n, t: fs.statSync(`${MAIL}/${n}`).mtimeMs })).sort((a, b) => b.t - a.t)[0].n;
    const text = fs.readFileSync(`${MAIL}/${f}`, 'utf8');
    return text.match(/es: (\d{6})/)[1];
  };
  const boot = async () => { await p.goto(`${BASE}/`, { waitUntil: 'load' }); await sleep(4500); await p.evaluate(() => document.querySelector('flt-semantics-placeholder')?.click()); await sleep(800); };

  await boot();
  step = 'abrir consola';
  await click('Crear cuenta y obtener');
  await waitFor('Crear tu cuenta o entra', 2000).catch(() => {});
  await waitFor('Entrar');
  step = 'tras paso 1'; console.log('1. pantalla de acceso OK');
  await shot('1-acceso');

  // --- registro
  await click('Crear cuenta', 0);
  await type('Correo', EMAIL);
  await click('Enviarme un código');
  await waitFor('Si el correo es válido');
  const code = latestCode();
  await type('Código', code);
  await type('Elige una contraseña', PASSWORD);
  await type('Repite la contraseña', PASSWORD);
  await click('Acepto las condiciones');
  await shot('2-registro');
  await click('Crear cuenta', -1);
  await waitFor('Proyectos', 20000);
  step = 'tras paso 2'; console.log('2. registro con contraseña y entrada OK');
  await shot('3-panel');

  // --- la sesión sobrevive a recargar
  await p.reload({ waitUntil: 'load' }); await sleep(4500); await p.evaluate(() => document.querySelector('flt-semantics-placeholder')?.click()); await sleep(1500);
  const afterReload = await has('Proyectos');
  step = 'tras paso 3'; console.log('3. tras recargar sigue dentro:', afterReload);
  if (!afterReload) throw new Error('la sesión no sobrevivió a la recarga');

  // --- cerrar sesión y entrar en dos pasos
  step = 'ir a Cuenta';
  await click('Cuenta', -1);
  await waitFor('Cambiar la contraseña');
  await shot('4-cuenta');
  step = 'cerrar sesión';
  await click('Cerrar sesión', 0);
  await waitFor('Entrar');
  step = 'tras paso 4'; console.log('4. cierre de sesión OK');
  await type('Correo', EMAIL);
  await type('Contraseña', PASSWORD);
  await click('Continuar');
  await waitFor('Volver');
  const loginCode = latestCode();
  await type('Código', loginCode);
  await shot('5-segundo-paso');
  await click('Entrar', -1);
  await waitFor('Proyectos', 20000);
  step = 'tras paso 5'; console.log('5. entrar con contraseña + código OK');

  // --- organización, proyecto y API key
  await type('Nombre de la organización', 'Acme Entregas');
  await click('Crear', 0);
  await waitFor('Acme Entregas');
  await click('Acme Entregas');
  await waitFor('Esta organización aún no tiene proyectos');
  await type('Nombre del proyecto nuevo', 'App de reparto');
  await click('Crear', 0);
  await waitFor('App de reparto');
  await click('App de reparto');
  await waitFor('Nueva API key');
  await shot('6-claves');
  await click('Nueva API key');
  await waitFor('Permisos');
  await type('Etiqueta (por ejemplo, backend de producción)', 'backend');
  await click('Entregas completas');
  await shot('7-nueva-clave');
  await click('Generar');
  await waitFor('Copia tu API key ahora');
  await sleep(500);
  const secret = createdSecret;
  step = 'tras paso 6'; console.log('6. API key creada por el servidor con la forma del contrato:', /^gsk_[0-9a-f]{32}\.[A-Za-z0-9_-]{43}$/.test(secret), secret.slice(0, 12) + '…');
  const api = await fetch(`${BASE}/api/v1/sessions?limit=1`, { headers: { Authorization: 'Bearer ' + secret } });
  console.log('   esa key autentica contra la API real (200 esperado):', api.status);
  await shot('8-secreto');
  await click('Ya la guardé');
  await sleep(500);
  const leaked = await p.evaluate((sec) => document.documentElement.innerText.includes(sec) || [...document.querySelectorAll('flt-semantics')].some((e) => (e.getAttribute('aria-label') || '').includes(sec) || (e.innerText || '').includes(sec)), secret);
  step = 'tras paso 7'; console.log('7. el secreto no queda en la pantalla tras cerrarlo:', !leaked);
  const stored = await p.evaluate((sec) => JSON.stringify([sessionStorage, localStorage, document.cookie]).includes(sec), secret);
  console.log('   el secreto no se guardó en el navegador:', !stored);
  step = 'Configuración';
  await click('Configuración');
  await waitFor('Conexión');
  await shot('9-configuracion');

  console.log('errores de consola del navegador:', issues.length, issues.slice(0, 4));
  await b.close();
})().catch(async (e) => { console.log('FALLO:', e.message); process.exit(1); });
