// Runs page.html in headless Chromium with ANGLE on Vulkan and prints the
// spike line in the same shape as the native spikes, plus the cost of one
// CDP screenshot per frame, which is how a browser renderer hands frames out.
//
// Usage: node run.mjs <width> <height> <frames>
// Env: PLAYWRIGHT_CORE, CHROME, THREE_DIR, ASSETS, OUT (defaults below).

import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const env = (k, d) => process.env[k] || d;
const require = createRequire(import.meta.url);
const { chromium } = require(env('PLAYWRIGHT_CORE',
  'C:/Projects/hypercn/node_modules/.pnpm/playwright-core@1.61.1/node_modules/playwright-core'));
const chrome = env('CHROME', 'C:/Users/Steve/AppData/Local/ms-playwright/chromium-1248/chrome-win64/chrome.exe');
const threeDir = env('THREE_DIR', 'C:/Projects/gemmy-nightshift/node_modules/.pnpm/three@0.185.1/node_modules/three');
const assets = env('ASSETS', 'F:/director-spikes/assets');
const out = env('OUT', 'F:/director-spikes/out');
const [width, height, frames] = process.argv.slice(2).map(Number);

const types = { '.html': 'text/html', '.js': 'text/javascript', '.glb': 'model/gltf-binary', '.hdr': 'application/octet-stream' };
const server = http.createServer((req, res) => {
  const url = decodeURIComponent(req.url.split('?')[0]);
  const file = url.startsWith('/three/') ? path.join(threeDir, url.slice(7))
    : url.startsWith('/assets/') ? path.join(assets, url.slice(8))
    : path.join(here, 'page.html');
  fs.readFile(file, (err, data) => {
    if (err) { res.writeHead(404); res.end(); return; }
    res.writeHead(200, { 'content-type': types[path.extname(file)] || 'application/octet-stream' });
    res.end(data);
  });
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));
const port = server.address().port;

const browser = await chromium.launch({
  executablePath: chrome,
  headless: true,
  args: ['--use-gl=angle', '--use-angle=vulkan', '--enable-gpu', '--ignore-gpu-blocklist', '--enable-features=Vulkan'],
});
const page = await browser.newPage({ viewport: { width, height } });
page.on('console', (m) => { if (m.type() === 'error') console.error('page:', m.text()); });
page.on('pageerror', (e) => console.error('page error:', e.message));
await page.goto(`http://127.0.0.1:${port}/?w=${width}&h=${height}&frames=${frames}`);
const r = await page.evaluate(() => window.spike);

const pct = (v, p) => { const s = [...v].sort((a, b) => a - b); return s[Math.round((s.length - 1) * p)]; };
// One CDP screenshot per frame for 30 frames: the hand-off a browser renderer pays.
const cdp = await page.context().newCDPSession(page);
const tshot = [];
for (let i = 0; i < 30; i++) {
  const a = performance.now();
  const shot = await cdp.send('Page.captureScreenshot', { format: 'png', captureBeyondViewport: false });
  tshot.push(performance.now() - a);
  if (i === 0) fs.writeFileSync(`${out}/threejs-${width}x${height}-060.png`, Buffer.from(shot.data, 'base64'));
}
console.error(`threejs gpu: ${r.gpu}`);
console.log(`threejs ${width}x${height} frames=${r.t3d.length} boot_ms=${r.bootMs.toFixed(0)} | 3d median ${pct(r.t3d, 0.5).toFixed(2)} p95 ${pct(r.t3d, 0.95).toFixed(2)} | readPixels median ${pct(r.tread, 0.5).toFixed(2)} p95 ${pct(r.tread, 0.95).toFixed(2)} | cdp png median ${pct(tshot, 0.5).toFixed(2)} p95 ${pct(tshot, 0.95).toFixed(2)}`);
await browser.close();
server.close();
