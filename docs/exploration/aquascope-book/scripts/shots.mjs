// node shots.mjs : screenshots of the prototype's Aquascope|RustViz toggle, light + dark.
import { createRequire } from 'module';
const require = createRequire('$WORK/browser/');
const { chromium } = require('playwright-core');
const BASE = 'http://localhost:8000/';
const OUT = '$WORK/shots/';
// [page, index of the .rv-aq-tabs box on that page, short name]
const TARGETS = [
  ['ch04-01-what-is-ownership.html', 5, 'ch04-01-add_suffix'],
  ['ch04-02-references-and-borrowing.html', 1, 'ch04-02-greet-refs'],
  ['ch04-04-slices.html', 0, 'ch04-04-first_word'],
];
const browser = await chromium.launch({ executablePath: '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', headless: true });
async function shot(page, box, path) {
  await box.evaluate(e => { e.scrollIntoView({ block: 'start' }); window.scrollBy(0, -90); });
  await page.waitForTimeout(400);
  const b = await box.boundingBox();
  const x = Math.max(0, b.x - 80);
  await page.screenshot({ path, clip: { x, y: Math.max(0, b.y - 8), width: Math.min(1280 - x, b.width + 160), height: Math.min(1400 - Math.max(0, b.y - 8), b.height + 16) } });
}
const blocked = new Set(); const errors = [];
for (const [theme, scheme] of [['light', 'light'], ['navy', 'dark']]) {
  const ctx = await browser.newContext({ viewport: { width: 1280, height: 1400 }, colorScheme: scheme, deviceScaleFactor: 2 });
  await ctx.addInitScript(t => { try { localStorage.setItem('mdbook-theme', t); } catch (e) {} }, theme);
  // Never let the page talk to anything but the local server.
  await ctx.route('**/*', r => { const u = r.request().url(); if (u.startsWith(BASE) || u.startsWith('data:')) return r.continue(); blocked.add(u); return r.abort(); });
  const page = await ctx.newPage();
  page.on('console', m => { if (m.type() === 'error') errors.push(theme + ': ' + m.text()); });
  page.on('pageerror', e => errors.push(theme + ': ' + String(e)));
  for (const [file, idx, name] of TARGETS) {
    await page.goto(BASE + file, { waitUntil: 'networkidle' });
    await page.waitForTimeout(800);
    const box = page.locator('.rv-aq-tabs').nth(idx);
    await box.scrollIntoViewIfNeeded();
    await page.waitForTimeout(400);
    await shot(page, box, OUT + name + '-aquascope-' + scheme + '.png');
    await box.locator('.rv-aq-tab[data-pane="rv"]').click();
    await page.waitForTimeout(1200);
    await shot(page, box, OUT + name + '-rustviz-' + scheme + '.png');
    // hover a tooltip trigger in the timeline to show the RustViz tooltip works
    const tl = box.locator('object.tl_panel');
    const bb = await tl.boundingBox();
    const pt = await tl.evaluate(o => { const d = o.contentDocument; const c = d && d.querySelector('circle.tooltip-trigger, .tooltip-trigger circle'); if (!c) return null; const r = c.getBoundingClientRect(); return { x: r.x + r.width / 2, y: r.y + r.height / 2 }; });
    if (pt && bb) {
      await page.mouse.move(bb.x + 1, bb.y + 1); await page.waitForTimeout(200);
      await page.mouse.move(bb.x + pt.x, bb.y + pt.y); await page.waitForTimeout(500);
      const tip = await page.evaluate(() => { const t = document.getElementById('svg_tooltip'); return t && t.style.display === 'block' ? t.textContent : null; });
      console.log(name, scheme, 'tooltip:', tip);
      const vb = await box.boundingBox();
      await page.screenshot({ path: OUT + name + '-rustviz-hover-' + scheme + '.png', clip: { x: Math.max(0, vb.x - 80), y: Math.max(0, vb.y - 20), width: Math.min(1280 - Math.max(0, vb.x - 80), vb.width + 160), height: vb.height + 120 } });
    }
  }
  // whole-page context shot: first tab box on ch04-01 with surrounding prose
  await page.goto(BASE + 'ch04-01-what-is-ownership.html', { waitUntil: 'networkidle' });
  await page.waitForTimeout(800);
  await page.locator('.rv-aq-tabs').nth(5).evaluate(e => e.scrollIntoView({ block: 'center' }));
  await page.waitForTimeout(400);
  await page.screenshot({ path: OUT + 'page-ch04-01-' + scheme + '.png' });
  await ctx.close();
}
await browser.close();
console.log('blocked external requests:', [...blocked]);
console.log('console errors:', errors);
