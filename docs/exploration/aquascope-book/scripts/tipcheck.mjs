// node tipcheck.mjs : tooltips + code underline still work on a scaled (fit) RustViz figure
import { createRequire } from 'module';
const require = createRequire('$WORK/browser/');
const { chromium } = require('playwright-core');
const browser = await chromium.launch({ executablePath: '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', headless: true });
const kill = setTimeout(async () => { console.log('TIMEOUT'); await browser.close(); process.exit(1); }, 60000);
const ctx = await browser.newContext({ viewport: { width: 1280, height: 1200 }, deviceScaleFactor: 2 });
await ctx.addInitScript(() => { try { localStorage.setItem('mdbook-theme', 'navy'); } catch (e) {} });
await ctx.route('**/*', r => r.request().url().startsWith('http://localhost:8000/') ? r.continue() : r.abort());
const page = await ctx.newPage();
await page.goto('http://localhost:8000/ch04-02-references-and-borrowing.html', { waitUntil: 'networkidle' });
await page.waitForTimeout(1500);
const card = page.locator('.rv-viz').nth(0);
await card.evaluate(e => { e.scrollIntoView({ block: 'start' }); window.scrollBy(0, -80); });
await page.waitForTimeout(500);
const tl = card.locator('object.tl_panel');
const bb = await tl.boundingBox();
console.log('timeline object box', JSON.stringify(bb));
const pts = await tl.evaluate(o => [...o.contentDocument.querySelectorAll('circle.tooltip-trigger, .tooltip-trigger circle')].slice(1, 6).map(c => { const r = c.getBoundingClientRect(); return { x: r.x + r.width / 2, y: r.y + r.height / 2 }; }));
for (const p of pts) {
  await page.mouse.move(bb.x + 2, bb.y + 2); await page.waitForTimeout(100);
  await page.mouse.move(bb.x + p.x, bb.y + p.y); await page.waitForTimeout(300);
  const r = await page.evaluate(() => { const t = document.getElementById('svg_tooltip'); const o = document.querySelector('.rv-viz object.code_panel'); const u = o.contentDocument.getElementsByClassName('emph').length; return [t && t.style.display === 'block' ? t.textContent.slice(0, 70) : null, u]; });
  console.log(JSON.stringify(r));
}
const b = await card.boundingBox();
await page.screenshot({ path: '$WORK/shots/tooltip-on-scaled-figure-dark.png', clip: { x: b.x - 10, y: b.y - 10, width: b.width + 20, height: b.height + 20 } });
clearTimeout(kill); await browser.close();
