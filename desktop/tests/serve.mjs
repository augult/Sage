#!/usr/bin/env node
// Browser-only regression fixture: serves production UI files unchanged except
// for a test-only script injected before main.js. This does not run a backend.
import { createServer } from 'node:http';
import { readFile } from 'node:fs/promises';
import { dirname, extname, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const ui = resolve(here, '../ui');
const types = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.woff2': 'font/woff2', '.svg': 'image/svg+xml', '.png': 'image/png' };
const server = createServer(async (req, res) => {
  try {
    const pathname = decodeURIComponent(new URL(req.url, 'http://localhost').pathname);
    const file = pathname === '/__fixture.js' ? resolve(here, 'fixture.js') : resolve(ui, `.${pathname === '/' ? '/index.html' : pathname}`);
    if (file !== resolve(here, 'fixture.js') && !file.startsWith(ui + sep)) { res.writeHead(403).end(); return; }
    let content = await readFile(file);
    if (file === resolve(ui, 'index.html')) content = content.toString().replace('<script type="module" src="js/main.js"></script>', '<script src="/__fixture.js"></script>\n    <script type="module" src="js/main.js"></script>');
    res.writeHead(200, { 'Content-Type': types[extname(file)] || 'application/octet-stream', 'Cache-Control': 'no-store' });
    res.end(content);
  } catch (error) { res.writeHead(error.code === 'ENOENT' ? 404 : 500).end('Fixture asset unavailable'); }
});
server.listen(4173, '0.0.0.0', () => console.log('Browser-only UI fixture: http://localhost:4173/?test (mock Tauri IPC; no backend)'));
