// node shots2.mjs : round-2 screenshots (cards, flags, dark mode, combined views)
import { createRequire } from 'module';
const require = createRequire('$WORK/browser/');
const { chromium } = require('playwright-core');
const BASE = 'http://localhost:8000/';
const OUT = '$WORK/shots/';
const browser = await chromium.launch({ executablePath: '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', headless: true });
const blocked = new Set(); const errors = []; const notes = [];
async function clipShot(page, el, path, pad = 12) {
  await el.evaluate(e => { e.scrollIntoView({ block: 'start' }); window.scrollBy(0, -80); });
  await page.waitForTimeout(500);
  const b = await el.boundingBox();
  const x = Math.max(0, b.x - pad), y = Math.max(0, b.y - pad);
  await page.screenshot({ path, clip: { x, y, width: Math.min(1280 - x, b.width + 2 * pad), height: Math.min(1600 - y, b.height + 2 * pad) } });
}
async function open(page, file) { await page.goto(BASE + file, { waitUntil: 'networkidle' }); await page.waitForTimeout(1200); }
async function lineInObj(page, obj, line, xFrac) {
  const b = await obj.boundingBox();
  return { x: b.x + b.width * xFrac, y: b.y + 85 + 30 * (line - 1) };
}
for (const [theme, scheme] of [['light', 'light'], ['navy', 'dark']]) {
  const ctx = await browser.newContext({ viewport: { width: 1280, height: 1600 }, colorScheme: scheme, deviceScaleFactor: 2 });
  await ctx.addInitScript(t => { try { localStorage.setItem('mdbook-theme', t); } catch (e) {} }, theme);
  await ctx.route('**/*', r => { const u = r.request().url(); if (u.startsWith(BASE) || u.startsWith('data:')) return r.continue(); blocked.add(u); return r.abort(); });
  const page = await ctx.newPage();
  page.on('console', m => { if (m.type() === 'error') errors.push(theme + ': ' + m.text().slice(0, 160)); });
  page.on('pageerror', e => errors.push(theme + ': ' + String(e).slice(0, 160)));

  // 1. tabs on an aquascope block (Aquascope default), both states + heights
  await open(page, 'ch04-01-what-is-ownership.html');
  let card = page.locator('.rv-viz').nth(2);
  const h0 = (await card.boundingBox()).height;
  await clipShot(page, card, OUT + 'tabs-add_suffix-aquascope-' + scheme + '.png');
  await card.locator('.rv-viz-tab[data-pane="1"]').click(); await page.waitForTimeout(700);
  const h1 = (await card.boundingBox()).height;
  await clipShot(page, card, OUT + 'tabs-add_suffix-rustviz-' + scheme + '.png');
  notes.push(scheme + ' add_suffix card height aquascope=' + h0 + ' rustviz=' + h1);
  // context shot
  await card.evaluate(e => { e.scrollIntoView({ block: 'center' }); });
  await page.waitForTimeout(400);
  await page.screenshot({ path: OUT + 'page-ch04-01-' + scheme + '.png', clip: { x: 0, y: 0, width: 1280, height: 1100 } });
  // 1b. dark-mode variants on the same card (RustViz pane showing)
  if (scheme === 'dark') {
    await page.evaluate(() => document.querySelectorAll('.rv-viz').forEach(v => { v.querySelectorAll('object').forEach(o => { const s = o.contentDocument && o.contentDocument.getElementById('rv-theme'); if (s) s.textContent = ''; }); v.querySelector('.rv-fig') && (v.querySelector('.rv-fig').style.background = '#f1f1f1'); }));
    await clipShot(page, card, OUT + 'dark-variant-untouched-' + scheme + '.png');
    await page.addStyleTag({ content: '.rv-fig object { filter: invert(0.88) hue-rotate(180deg); } .rv-fig { background: transparent !important; }' });
    await page.waitForTimeout(300);
    await clipShot(page, card, OUT + 'dark-variant-css-filter-' + scheme + '.png');
  }

  // 2. rustviz-only and rustviz-default (ch04-02)
  await open(page, 'ch04-02-references-and-borrowing.html');
  await clipShot(page, page.locator('.rv-viz').nth(0), OUT + 'flag-rustviz-only-greet-' + scheme + '.png');
  await clipShot(page, page.locator('.rv-viz').nth(2), OUT + 'flag-rustviz-default-num-' + scheme + '.png');
  card = page.locator('.rv-viz').nth(3);
  await card.locator('.rv-viz-tab[data-pane="1"]').click(); await page.waitForTimeout(700);
  await clipShot(page, card, OUT + 'flag-rustviz-ascii_capitalize-rustviz-' + scheme + '.png');

  // 3. rust listing blocks (ch04-03 add_big_strings; ch04-04 Listing; ch04-05 rustviz-only)
  await open(page, 'ch04-03-fixing-ownership-errors.html');
  for (const i of [0, 2]) {
    card = page.locator('.rv-viz').nth(i);
    if (i === 2) await clipShot(page, card, OUT + 'rust-block-code-tab-' + scheme + '.png');
    await card.locator('.rv-viz-tab[data-pane="1"]').click(); await page.waitForTimeout(700);
    await clipShot(page, card, OUT + 'rust-block-add_big_strings-' + i + '-rustviz-' + scheme + '.png');
  }
  await open(page, 'ch04-04-slices.html');
  await clipShot(page, page.locator('.rv-viz').nth(3), OUT + 'rust-listing-first_word-rustviz-default-' + scheme + '.png', 30);
  await open(page, 'ch04-05-ownership-recap.html');
  await clipShot(page, page.locator('.rv-viz').nth(0), OUT + 'rust-block-document-rustviz-only-' + scheme + '.png');

  // 4. combined views
  await open(page, 'rv-combined.html');
  await page.waitForTimeout(1500);
  const combos = page.locator('.rv-combo');
  const n = await combos.count();
  const names = ['add_suffix', 'greet', 'vec_push'];
  const hoverLines = [[2, 8, 9], [3, 4, 9], [3, 4, 5]];
  for (let i = 0; i < n; i++) {
    const c = combos.nth(i);
    const kind = await c.getAttribute('data-combo');
    const ex = names[Math.floor(i / 2)];
    const tl = c.locator('object.tl_panel');
    await c.evaluate(e => { e.scrollIntoView({ block: 'start' }); window.scrollBy(0, -80); });
    await page.waitForTimeout(400);
    if (kind === 'hover') {
      for (const ln of hoverLines[Math.floor(i / 2)]) {
        const p = await lineInObj(page, tl, ln, 0.5);
        await page.mouse.move(p.x, p.y); await page.waitForTimeout(600);
        const cap = await c.locator('.rv-combo-cap').textContent();
        notes.push(scheme + ' ' + ex + ' hover line ' + ln + ': ' + cap);
        await clipShot(page, c, OUT + 'combo-A-hover-' + ex + '-line' + ln + '-' + scheme + '.png');
      }
    } else {
      await clipShot(page, c, OUT + 'combo-B-rows-' + ex + '-' + scheme + '.png');
      // hover the 2nd snapshot to show the link
      const st = c.locator('.rv-combo-aq .interpreter .step').nth(1);
      if (await st.count()) {
        await st.hover(); await page.waitForTimeout(500);
        await clipShot(page, c, OUT + 'combo-B-rows-' + ex + '-hoverL2-' + scheme + '.png');
        await page.mouse.move(5, 5);
      }
    }
  }
  await ctx.close();
}
await browser.close();
console.log(notes.join('\n'));
console.log('blocked external requests:', [...blocked]);
console.log('console errors:', [...new Set(errors)].filter(e => !e.includes('unique "key"')));
