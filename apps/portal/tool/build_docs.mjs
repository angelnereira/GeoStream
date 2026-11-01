// Genera la documentación publicada del portal a partir de `docs-manifest.txt` (ADR-0034 §2).
// Uso: node build_docs.mjs <raíz del repositorio> <directorio de salida>
// Requiere `marked` (se instala en la imagen de construcción, no es una dependencia de la app).
import { copyFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { basename, join } from 'node:path';
import { marked } from 'marked';

const [root, out] = process.argv.slice(2);
if (!root || !out) {
  console.error('uso: node build_docs.mjs <raíz> <salida>');
  process.exit(2);
}

const entries = readFileSync(join(root, 'apps/portal/docs-manifest.txt'), 'utf8')
  .split('\n')
  .map((line) => line.trim())
  .filter((line) => line && !line.startsWith('#'))
  .map((line) => {
    const [kind, path] = line.split(/\s+/);
    return { kind, path };
  });

const docs = entries.filter((e) => e.kind === 'doc');
const protos = entries.filter((e) => e.kind === 'proto');
const published = new Set(docs.map((d) => basename(d.path)));

const escapeHtml = (text) =>
  text.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');

const page = (title, body) => `<!doctype html>
<html lang="es"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>${escapeHtml(title)} — GeoStream</title>
<link rel="stylesheet" href="/docs/docs.css"></head>
<body><header><a href="/">← GeoStream</a></header><main>${body}</main></body></html>`;

mkdirSync(join(out, 'docs'), { recursive: true });
mkdirSync(join(out, 'contracts'), { recursive: true });

// Enlaces entre documentos: los publicados apuntan a su HTML; el resto queda como texto.
const rewriteLinks = (html) =>
  html.replace(/<a href="([^"]*)">([\s\S]*?)<\/a>/g, (whole, href, text) => {
    if (/^(https?:|mailto:|#|\/)/.test(href)) return whole;
    const [file, anchor] = href.split('#');
    const name = basename(file);
    if (published.has(name)) {
      return `<a href="/docs/${name.replace(/\.md$/, '.html')}${anchor ? `#${anchor}` : ''}">${text}</a>`;
    }
    return text;
  });

for (const doc of docs) {
  const markdown = readFileSync(join(root, doc.path), 'utf8');
  const title = (markdown.match(/^#\s+(.+)$/m) ?? [, basename(doc.path)])[1];
  const html = rewriteLinks(marked.parse(markdown, { gfm: true }));
  writeFileSync(
    join(out, 'docs', basename(doc.path).replace(/\.md$/, '.html')),
    page(title, html),
  );
}

for (const entry of entries.filter((e) => e.kind === 'proto' || e.kind === 'openapi')) {
  copyFileSync(join(root, entry.path), join(out, 'contracts', basename(entry.path)));
}

const list = [
  ...protos.map((p) => basename(p.path)),
  ...entries.filter((e) => e.kind === 'openapi').map((e) => basename(e.path)),
]
  .map((name) => `<li><a href="/contracts/${name}">${name}</a></li>`)
  .join('');
writeFileSync(
  join(out, 'contracts', 'index.html'),
  page('Contratos', `<h1>Contratos</h1><p>Archivos fuente de los contratos, idénticos a los del repositorio.</p><ul>${list}</ul>`),
);

// Referencia de la API: Redoc alojado en el propio portal (sin CDN, sin fuentes de terceros), que
// lee el OpenAPI publicado en /contracts/. Así la CSP puede ser `script-src 'self'`.
const redocBundle = join(process.cwd(), 'node_modules/redoc/bundles/redoc.standalone.js');
if (!existsSync(redocBundle)) {
  console.error(`falta ${redocBundle}: instala redoc en el directorio de trabajo`);
  process.exit(1);
}
copyFileSync(redocBundle, join(out, 'docs', 'redoc.standalone.js'));
writeFileSync(
  join(out, 'docs', 'api.html'),
  `<!doctype html>
<html lang="es"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>Referencia de la API — GeoStream</title></head>
<body><div id="redoc"></div>
<script src="/docs/redoc.standalone.js"></script>
<script src="/docs/api.js"></script></body></html>`,
);
writeFileSync(
  join(out, 'docs', 'api.js'),
  `Redoc.init('/contracts/geostream.v1.yaml', {
  hideHostname: false,
  theme: { typography: { fontFamily: 'system-ui, sans-serif', headings: { fontFamily: 'system-ui, sans-serif' }, code: { fontFamily: 'ui-monospace, monospace' } } },
}, document.getElementById('redoc'));`,
);

writeFileSync(
  join(out, 'docs', 'docs.css'),
  `body{font:16px/1.6 system-ui,sans-serif;margin:0;color:#1b1b1b;background:#fff}
@media (prefers-color-scheme:dark){body{color:#e6e6e6;background:#121212}a{color:#7fc4a5}}
header{padding:12px 16px;border-bottom:1px solid #8884}main{max-width:860px;margin:0 auto;padding:16px}
pre,code{font-family:ui-monospace,monospace;font-size:.9em}pre{overflow:auto;padding:12px;background:#8881;border-radius:6px}
table{border-collapse:collapse;display:block;overflow:auto}td,th{border:1px solid #8886;padding:4px 8px}`,
);
console.log(`docs: ${docs.length} documentos, ${protos.length} .proto`);
