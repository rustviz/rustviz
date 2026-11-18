// Run: npx tsx scripts/telemetry.test.ts
// Unit tests for the usage-logging client (public/ex-assets/rv-telemetry.js,
// run here in a node `vm` with a minimal fake DOM) and the playground's
// helpers in src/telemetry.ts. The browser behaviour (real figures, real
// network) is checked separately with headless Chrome.
import assert from 'node:assert/strict';
import { createHash, webcrypto } from 'node:crypto';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import { classifyOutcome, crashLocation, errorCodes, TELEMETRY_ENABLED } from '../src/telemetry';

const SRC = readFileSync(new URL('../public/ex-assets/rv-telemetry.js', import.meta.url), 'utf8');
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;

// ------------------------------------------------------------ fake DOM

type Listener = (e: unknown) => void;
class FakeEl {
  tagName: string;
  children: FakeEl[] = [];
  attrs: Record<string, string> = {};
  listeners: Record<string, Listener[]> = {};
  textContent = '';
  hidden = false;
  style: Record<string, string> = {};
  parentElement: FakeEl | null = null;
  classList: { contains: (c: string) => boolean; [i: number]: string; length: number };
  constructor(tag: string, classes: string[] = []) {
    this.tagName = tag.toUpperCase();
    const cl = classes.slice() as unknown as FakeEl['classList'];
    cl.contains = (c: string) => classes.includes(c);
    this.classList = cl;
  }
  setAttribute(k: string, v: string) { this.attrs[k] = String(v); }
  getAttribute(k: string) { return k in this.attrs ? this.attrs[k] : null; }
  getAttributeNS(_: unknown, k: string) { return this.getAttribute(k); }
  appendChild(c: FakeEl) { c.parentElement = this; this.children.push(c); return c; }
  insertBefore(c: FakeEl, before: FakeEl | null) {
    c.parentElement = this;
    const i = before ? this.children.indexOf(before) : -1;
    if (i < 0) this.children.push(c); else this.children.splice(i, 0, c);
    return c;
  }
  get firstChild() { return this.children[0] ?? null; }
  hasAttribute(k: string) { return k in this.attrs; }
  removeAttribute(k: string) { delete this.attrs[k]; }
  remove() {
    const p = this.parentElement;
    if (p) p.children = p.children.filter(c => c !== this);
  }
  addEventListener(t: string, fn: Listener) { (this.listeners[t] ||= []).push(fn); }
  removeEventListener() {}
  focus() {}
  closest(sel: string): FakeEl | null {
    let e: FakeEl | null = this;
    while (e) {
      if (sel === '.vis_block' && e.classList.contains('vis_block')) return e;
      e = e.parentElement;
    }
    return null;
  }
  querySelector(sel: string): FakeEl | null {
    for (const c of this.children) {
      if (sel === c.tagName.toLowerCase()) return c;
      const r = c.querySelector(sel);
      if (r) return r;
    }
    return null;
  }
  querySelectorAll(): FakeEl[] { return []; }
  text(): string { return this.textContent + this.children.map(c => c.text()).join(' '); }
}

type Env = {
  win: Record<string, any>;
  beacons: string[];
  fetches: string[];
  local: Map<string, string>;
  body: FakeEl;
  figures: FakeEl[];
  docListeners: Record<string, Listener[]>;
  winListeners: Record<string, Listener[]>;
  els: Record<string, FakeEl>;
};

function storage(m: Map<string, string>) {
  return {
    getItem: (k: string) => (m.has(k) ? m.get(k)! : null),
    setItem: (k: string, v: string) => { m.set(k, String(v)); },
    removeItem: (k: string) => { m.delete(k); },
  };
}

function boot(config: Record<string, unknown> | undefined, opts: {
  dnt?: boolean; gpc?: boolean; search?: string; local?: Map<string, string>; figures?: FakeEl[];
  slots?: Record<string, FakeEl>;
  // the privacy page: its inline script, and what telemetry-config.json holds
  // (null: no such file in this build)
  page?: string; configJson?: Record<string, unknown> | null;
} = {}): Env {
  const beacons: string[] = [];
  const fetches: string[] = [];
  const local = opts.local ?? new Map<string, string>();
  const body = new FakeEl('body');
  const head = new FakeEl('head');
  const figures = opts.figures ?? [];
  const docListeners: Record<string, Listener[]> = {};
  const winListeners: Record<string, Listener[]> = {};
  const els: Record<string, FakeEl> = {};
  for (const id of ['yd-status', 'yd-id', 'yd-button']) els[id] = new FakeEl(id === 'yd-button' ? 'button' : 'p');
  const document = {
    readyState: 'complete',
    visibilityState: 'visible',
    referrer: '',
    body, head, documentElement: new FakeEl('html'),
    activeElement: null,
    hasFocus: () => true,
    createElement: (t: string) => new FakeEl(t),
    createTextNode: (t: string) => { const e = new FakeEl('#text'); e.textContent = t; return e; },
    getElementById: (id: string) => els[id] ?? null,
    querySelector: (sel: string) => (opts.slots ?? {})[sel] ?? null,
    querySelectorAll: (sel: string) => (sel === '.tl_panel' ? figures : []),
    addEventListener: (t: string, fn: Listener) => { (docListeners[t] ||= []).push(fn); },
  };
  const win: Record<string, any> = {
    RV_TELEMETRY_CONFIG: config,
    __RV_TELEMETRY_TEST__: true,
    document,
    location: { pathname: '/tutorial/ch04-01.html', search: opts.search ?? '', hostname: 'rustviz.github.io' },
    navigator: {
      language: 'en-US',
      doNotTrack: opts.dnt ? '1' : null,
      globalPrivacyControl: !!opts.gpc,
      sendBeacon: (_url: string, body: string) => { beacons.push(body); return true; },
    },
    localStorage: storage(local),
    sessionStorage: storage(new Map()),
    crypto: webcrypto,
    TextEncoder, URL, JSON, Math, Date, Object, Array, Uint8Array, DataView, parseInt, parseFloat, isFinite,
    performance: { getEntriesByType: () => [{ type: 'navigate' }] },
    fetch: (url: string, init: { body: string }) => {
      if (/telemetry-config\.json$/.test(url)) {
        const c = opts.configJson ?? null;
        return Promise.resolve({ ok: c !== null, status: c ? 200 : 404, json: () => Promise.resolve(c) });
      }
      fetches.push(init.body);
      return Promise.resolve({ status: 200 });
    },
    setInterval: () => 0,
    setTimeout: () => 0,
    addEventListener: (t: string, fn: Listener) => { (winListeners[t] ||= []).push(fn); },
    innerWidth: 1234, innerHeight: 777, scrollY: 0,
  };
  win.window = win;
  vm.createContext(win);
  if (opts.page !== undefined) {
    // the privacy page: its script adds rv-telemetry.js with a <script>
    head.appendChild = (c: FakeEl) => {
      FakeEl.prototype.appendChild.call(head, c);
      if (c.tagName === 'SCRIPT') {
        vm.runInContext(SRC, win);
        (c as any).onload?.({});
      }
      return c;
    };
    vm.runInContext(opts.page, win);
  } else vm.runInContext(SRC, win);
  return { win, beacons, fetches, local, body, figures, docListeners, winListeners, els };
}

const t = (env: Env) => env.win.rvTelemetry;
// (a JSON copy: the vm's arrays and objects are from another realm)
const q = (env: Env): any[] => JSON.parse(JSON.stringify(t(env)._test.queue));
const names = (env: Env) => q(env).map((e: any) => e.event);

// The collector's envelope rules (schema 1), enough to catch drift.
function checkEnvelope(ev: any) {
  assert.equal(ev.schema, 1);
  assert.match(ev.event_id, UUID);
  assert.match(ev.page_load_id, UUID);
  assert.match(ev.session_id, UUID);
  assert.match(ev.anon_id, UUID);
  assert.ok(Number.isInteger(ev.seq) && ev.seq >= 0);
  assert.ok(ev.client_ts > 1.6e12 && ev.client_ts < 1e13);
  assert.match(ev.surface, /^(tutorial|playground|book:[a-z0-9.-]{1,64})$/);
  assert.match(ev.page, /^\/[^?#]{0,255}$/);
  assert.ok(ev.consent_mode === 'notice' || ev.consent_mode === 'opt_in');
  assert.ok(ev.tier === 1 || ev.tier === 2);
  assert.match(ev.rv_version, /^[0-9A-Za-z.+-]{1,32}$/);
  assert.match(ev.client_version, /^[0-9A-Za-z.+-]{1,32}$/);
  assert.match(ev.viewport, /^[0-9]{1,5}x[0-9]{1,5}$/);
  assert.ok(ev.tz_offset >= -840 && ev.tz_offset <= 840);
  assert.equal(typeof ev.props, 'object');
  if (ev.tier === 1) assert.equal(ev.cohort, undefined);
}

const CFG = { endpoint: 'http://127.0.0.1:8090/', rvVersion: '2.1.0', surface: 'tutorial',
              privacyUrl: 'https://rustviz.github.io/privacy/' };

// --- no endpoint: nothing at all
for (const cfg of [undefined, {}, { endpoint: '' }, { endpoint: 42 }]) {
  const env = boot(cfg as Record<string, unknown> | undefined);
  assert.equal(env.win.rvTelemetry, undefined, JSON.stringify(cfg));
  assert.equal(env.body.children.length, 0);
}

// --- sha256 / figure ids (same formula as mdbook-rustviz's figure_id)
{
  const env = boot(CFG);
  const sha = t(env)._test.sha256;
  for (const s of ['', 'abc', 'héllo 🦀\n', 'x'.repeat(1000)]) {
    assert.equal(sha(s), createHash('sha256').update(s, 'utf8').digest('hex'), s.slice(0, 10));
  }
  assert.equal(t(env).figureId('abc'), 'fd7e25f9ee13a174'); // = mdbook-rustviz's test vector
  assert.equal(t(env).programId('fn main() {}'), createHash('sha256').update('fn main() {}').digest('hex'));
}

// --- opt-in (the default): nothing before the visitor says yes
{
  const env = boot(CFG);
  assert.equal(t(env).isActive(), false);
  assert.deepEqual(names(env), []);
  assert.equal(t(env).emit('ui', { action: 'other' }), null);
  assert.equal(env.local.get('rv.telemetry.anon_id'), undefined, 'no id before consent');
  // the bar goes first in the page's flow; the "Your data" link last
  assert.equal(env.body.children.length, 2, 'bar and footer link');
  assert.match(env.body.children[0].attrs.class, /rv-tlm-bar/);
  assert.match(env.body.children[1].text(), /Your data/);
  const yd = (function find(e: FakeEl): FakeEl | null {
    return (e.tagName === 'A' && e.textContent === 'Your data') ? e : e.children.reduce<FakeEl | null>((r, c) => r || find(c), null);
  })(env.body.children[1]);
  assert.equal(yd!.attrs.href, 'https://rustviz.github.io/privacy/#your-data', 'a plain link to the privacy page section');
  assert.match(env.body.children[0].text(), /May this site record/);
  assert.match(env.body.children[0].text(), /No name, e-mail, IP address or cookies/);
  t(env)._test.optIn(false);
  // saying yes sends right away (fetch, not beacon) and empties the queue
  assert.equal(q(env).length, 0);
  assert.equal(env.fetches.length, 1);
  const sent = JSON.parse(env.fetches[0]).events;
  assert.deepEqual(sent.map((e: any) => e.event), ['consent', 'page_view']);
  sent.forEach(checkEnvelope);
  assert.deepEqual(sent.map((e: any) => e.seq), [0, 1]);
  assert.equal(sent[0].props.choice, 'opt_in');
  assert.equal(sent[0].consent_mode, 'opt_in');
  assert.equal(sent[1].props.nav_type, 'navigate');
  assert.equal(sent[0].page, '/tutorial/ch04-01.html');
  // the choice persists: a later page load is on without asking again
  const again = boot(CFG, { local: env.local });
  assert.equal(t(again).isActive(), true);
  assert.deepEqual(names(again), ['page_view']);
  assert.equal(q(again)[0].anon_id, env.local.get('rv.telemetry.anon_id'));
}

// --- the bar goes in the page's slot when it has one (the playground's
//     layout row), never over the content; footLink 'none' adds no link
{
  const slot = new FakeEl('div');
  const env = boot({ ...CFG, consent: 'opt-in', bannerSlot: '#slot', footLink: 'none' }, { slots: { '#slot': slot } });
  assert.equal(slot.children.length, 1);
  assert.match(slot.children[0].attrs.class, /rv-tlm-bar/);
  assert.doesNotMatch(slot.children[0].attrs.class, /boxed/);
  assert.equal(env.body.children.length, 0);
  // in mdBook it is the first thing in <main>
  const main = new FakeEl('main');
  main.appendChild(new FakeEl('h1'));
  const book = boot({ ...CFG, consent: 'opt-in' }, { slots: { main } });
  assert.match(main.children[0].attrs.class, /rv-tlm-bar rv-tlm-boxed/);
  assert.match(main.children[main.children.length - 1].text(), /Your data/);
}

// --- declining stores no id and is never asked again
{
  const env = boot(CFG);
  t(env)._test.decline();
  assert.deepEqual(names(env), []);
  const again = boot(CFG, { local: env.local });
  assert.equal(t(again).isActive(), false);
  assert.equal(again.local.get('rv.telemetry.anon_id'), undefined);
  assert.ok(!again.body.children.some(c => /May this site/.test(c.text())), 'no banner after declining');
}

// --- notice mode: no banner anywhere; events flow without a click
{
  const env = boot({ ...CFG, consent: 'notice' });
  assert.equal(t(env).isActive(), true);
  assert.deepEqual(names(env), ['page_view']);
  assert.equal(q(env)[0].consent_mode, 'notice');
  q(env).forEach(checkEnvelope);
  assert.equal(env.body.children.length, 0, 'notice mode renders no banner');
  assert.equal(env.local.get('rv.telemetry.choice'), undefined, 'nothing stored just for viewing');
  const again = boot({ ...CFG, consent: 'notice' }, { local: env.local });
  assert.deepEqual(names(again), ['page_view']);
  // a visitor whose browser stored the older 'notice_shown' choice is on
  const old = boot({ ...CFG, consent: 'notice' }, { local: new Map([
    ['rv.telemetry.choice', JSON.stringify({ state: 'on', how: 'notice_shown', version: '2026-11' })]]) });
  assert.equal(t(old).isActive(), true);
  assert.equal(old.body.children.length, 0);
  assert.ok(!names(old).includes('consent'));
  // a site that switches to opt-in asks again (and shows its prompt)
  const optin = boot(CFG, { local: env.local });
  assert.equal(t(optin).isActive(), false);
  assert.match(optin.body.children[0].text(), /May this site record/);
}

// --- opt-in mode: its prompt is shown and nothing is sent before a yes
{
  const env = boot({ ...CFG, consent: 'opt-in' });
  assert.match(env.body.children[0].text(), /May this site record/);
  assert.equal(env.fetches.length + env.beacons.length, 0);
  assert.deepEqual(names(env), []);
}

// --- no pop-up in either mode: no dialog, no panel API, no click hook
{
  const hasPopup = (e: FakeEl): boolean =>
    e.attrs.role === 'dialog' || /rv-tlm-panel/.test(e.attrs.class ?? '') || e.children.some(hasPopup);
  for (const consent of ['notice', 'opt-in']) {
    const env = boot({ ...CFG, consent });
    assert.equal(t(env).openPanel, undefined, consent);
    assert.equal(env.docListeners.click.length, 1, 'only the figure click listener, no #rv-your-data hook');
    // the hook the book's own text used to use does nothing now
    const link = new FakeEl('a');
    link.setAttribute('href', '#rv-your-data');
    env.docListeners.click.forEach(fn => fn({ target: link, preventDefault: () => { throw new Error('hooked'); } }));
    assert.ok(!hasPopup(env.body));
    t(env)._test.withdraw();
    t(env)._test.turnOn();
    assert.ok(!hasPopup(env.body), 'not after turning off and on either');
  }
}

// --- withdraw: one last event with the old id, then nothing and no ids
{
  const env = boot({ ...CFG, consent: 'notice' });
  const anon = env.local.get('rv.telemetry.anon_id');
  assert.match(anon!, UUID);
  t(env)._test.withdraw();
  const sent = env.beacons.flatMap(b => JSON.parse(b).events);
  const w = sent[sent.length - 1];
  assert.equal(w.event, 'withdraw');
  assert.equal(w.anon_id, anon);
  assert.deepEqual(JSON.parse(JSON.stringify(w.props)), { consent_version: '2026-11' });
  assert.equal(env.local.get('rv.telemetry.anon_id'), undefined);
  assert.equal(env.local.get('rv.telemetry.session'), undefined);
  assert.equal(t(env).isActive(), false);
  assert.equal(t(env).emit('ui', { action: 'other' }), null);
  const again = boot({ ...CFG, consent: 'notice' }, { local: env.local });
  assert.equal(t(again).isActive(), false, 'stays off on later visits');
  assert.deepEqual(names(again), []);
}

// --- the privacy page's "Your data": control-only client + the page's script
const PAGE = (() => {
  const html = readFileSync(new URL('../public/privacy/index.html', import.meta.url), 'utf8');
  return /<script>([\s\S]*?)<\/script>/.exec(html)![1];
})();
const settle = async () => { for (let i = 0; i < 8; i++) await new Promise(r => setImmediate(r)); };
const PCFG = { endpoint: 'http://127.0.0.1:8090', consent: 'notice', rvVersion: '2.1.0' };
const allEvents = (e: Env) => [...e.beacons, ...e.fetches].flatMap(x => JSON.parse(x).events as any[]);
const click = (el: FakeEl) => el.listeners.click.forEach(fn => fn({}));
{
  // a tutorial page is open and recording; the visitor then opens the privacy page
  const local = new Map<string, string>();
  const tut = boot({ ...CFG, consent: 'notice' }, { local });
  const oldId = local.get('rv.telemetry.anon_id')!;
  assert.match(oldId, UUID);
  const pg = boot(undefined, { local, page: PAGE, configJson: PCFG });
  await settle();
  const { els } = pg;
  assert.equal(els['yd-status'].textContent, 'Recording is on.');
  assert.match(els['yd-id'].text(), new RegExp(oldId));
  assert.equal(els['yd-button'].textContent, 'Turn off');
  assert.equal(els['yd-button'].hidden, false);
  assert.equal(pg.body.children.length, 0, 'no popup, no banner on the privacy page');
  assert.equal(allEvents(pg).length, 0, 'the privacy page records nothing by being opened');
  assert.deepEqual(q(pg), []);

  click(els['yd-button']);   // Turn off
  const evs = allEvents(pg);
  assert.equal(evs.length, 1, 'exactly one event');
  assert.equal(evs[0].event, 'withdraw');
  assert.equal(evs[0].anon_id, oldId, 'carries the id being deleted');
  assert.equal(evs[0].surface, 'playground');
  assert.equal(evs[0].page, '/tutorial/ch04-01.html'); // (the fake location is shared)
  checkEnvelope(evs[0]);
  assert.equal(local.get('rv.telemetry.anon_id'), undefined);
  assert.equal(local.get('rv.telemetry.session'), undefined);
  assert.equal(JSON.parse(local.get('rv.telemetry.choice')!).state, 'off');
  assert.equal(els['yd-status'].textContent, 'Recording is off. Nothing is recorded.');
  assert.match(els['yd-id'].text(), /none stored/);
  assert.equal(els['yd-button'].textContent, 'Turn on');
  assert.equal(pg.fetches.length + pg.beacons.length, 1, 'nothing more after that');

  // the already-open tutorial page hears about it (storage event) and stops
  const before = tut.beacons.length + tut.fetches.length;
  tut.winListeners.storage.forEach(fn => fn({ key: 'rv.telemetry.choice' }));
  assert.equal(t(tut).isActive(), false);
  assert.equal(t(tut).emit('ui', { action: 'other' }), null);
  t(tut)._test.pageHide('hidden');
  t(tut)._test.flush(false);
  assert.equal(tut.beacons.length + tut.fetches.length, before, 'the open page sends nothing afterwards');
  assert.equal(local.get('rv.telemetry.anon_id'), undefined, 'and made no new id');
  // a page loaded later is off too
  assert.equal(t(boot({ ...CFG, consent: 'notice' }, { local })).isActive(), false);
  // another storage key does not matter
  tut.winListeners.storage.forEach(fn => fn({ key: 'something.else' }));
  assert.equal(t(tut).isActive(), false);

  // Turn off again (nothing to withdraw): no second event
  const pg2 = boot(undefined, { local, page: PAGE, configJson: PCFG });
  await settle();
  assert.equal(pg2.els['yd-button'].textContent, 'Turn on');

  // Turn on: recording restarts with a new ID
  click(pg2.els['yd-button']);
  assert.equal(pg2.els['yd-status'].textContent, 'Recording is on.');
  assert.match(pg2.els['yd-id'].text(), /none yet/);
  assert.equal(pg2.els['yd-button'].textContent, 'Turn off');
  assert.equal(allEvents(pg2).length, 0, 'turning on sends nothing from the privacy page');
  tut.winListeners.storage.forEach(fn => fn({ key: 'rv.telemetry.choice' }));
  assert.equal(t(tut).isActive(), true, 'the open page resumes');
  assert.ok(names(tut).includes('page_view'));
  const next = boot({ ...CFG, consent: 'notice' }, { local });
  assert.equal(t(next).isActive(), true);
  const newId = local.get('rv.telemetry.anon_id')!;
  assert.match(newId, UUID);
  assert.notEqual(newId, oldId, 'a new ID');
  assert.equal(q(next)[0].anon_id, newId);
  // turning off with a recording page elsewhere sends one withdraw with the new id
  const pg3 = boot(undefined, { local, page: PAGE, configJson: PCFG });
  await settle();
  click(pg3.els['yd-button']);
  assert.deepEqual(allEvents(pg3).map(e => [e.event, e.anon_id]), [['withdraw', newId]]);
}

// --- privacy page, GPC / DNT: nothing recorded, no button, nothing sent
for (const sig of [{ gpc: true }, { dnt: true }]) {
  const pg = boot(undefined, { ...sig, page: PAGE, configJson: PCFG });
  await settle();
  assert.match(pg.els['yd-status'].textContent, /nothing is recorded and no ID is stored/);
  assert.equal(pg.els['yd-button'].hidden, true);
  assert.equal(pg.els['yd-id'].hidden, true);
  assert.equal(pg.fetches.length + pg.beacons.length, 0);
  assert.equal(pg.local.size, 0);
  // even calling the control API directly does nothing
  pg.win.rvTelemetry.control.turnOn();
  pg.win.rvTelemetry.control.turnOff();
  assert.equal(pg.fetches.length + pg.beacons.length, 0);
}

// --- privacy page in a build without an endpoint: says so, no button
for (const configJson of [null, { endpoint: '' }]) {
  const pg = boot(undefined, { page: PAGE, configJson });
  await settle();
  assert.match(pg.els['yd-status'].textContent, /not enabled/);
  assert.equal(pg.els['yd-button'].hidden, true);
  assert.equal(pg.els['yd-id'].hidden, true);
  assert.equal(pg.win.rvTelemetry, undefined, 'the client is not even loaded');
  assert.equal(pg.fetches.length + pg.beacons.length, 0);
}

// --- privacy page, opt-in build: Turn on is an opt-in; other pages then record
{
  const local = new Map<string, string>();
  const pg = boot(undefined, { local, page: PAGE, configJson: { ...PCFG, consent: 'opt-in' } });
  await settle();
  assert.equal(pg.els['yd-status'].textContent, 'Recording is off. Nothing is recorded.');
  click(pg.els['yd-button']);
  assert.equal(pg.els['yd-status'].textContent, 'Recording is on.');
  const tut = boot({ ...CFG, consent: 'opt-in' }, { local });
  assert.equal(t(tut).isActive(), true);
}

// --- Do Not Track / Global Privacy Control: nothing, in every mode
for (const sig of [{ dnt: true }, { gpc: true }]) {
  for (const consent of ['notice', 'opt-in']) {
    const env = boot({ ...CFG, consent }, sig);
    assert.equal(t(env).isActive(), false);
    assert.deepEqual(names(env), []);
    assert.equal(env.body.children.length, 0, 'no banner');
    assert.equal(env.local.size, 0, 'nothing stored');
    assert.equal(env.fetches.length + env.beacons.length, 0);
  }
}

// --- respectPrivacySignals: false (the RustViz sites): GPC / DNT do not stop logging
for (const sig of [{ dnt: true }, { gpc: true }]) {
  const cfg = { ...CFG, consent: 'notice', respectPrivacySignals: false };
  const env = boot(cfg, sig);
  assert.equal(t(env).isActive(), true, 'recording despite the signal');
  assert.ok(names(env).includes('page_view'), 'events flow');
  assert.match(env.local.get('rv.telemetry.anon_id')!, UUID);
  // Turn off still stops it, and stays off on the next visit
  t(env)._test.withdraw();
  assert.equal(t(env).isActive(), false);
  assert.equal(env.local.get('rv.telemetry.anon_id'), undefined);
  const again = boot(cfg, { ...sig, local: env.local });
  assert.equal(t(again).isActive(), false, 'stays off');
  assert.deepEqual(names(again), []);
}
// an explicit true is the default
{
  const env = boot({ ...CFG, consent: 'notice', respectPrivacySignals: true }, { gpc: true });
  assert.equal(t(env).isActive(), false);
  assert.equal(env.fetches.length + env.beacons.length, 0);
}

// --- privacy page, our config (respectPrivacySignals false) + GPC / DNT: recording is on, button works
for (const sig of [{ gpc: true }, { dnt: true }]) {
  const local = new Map<string, string>();
  const pg = boot(undefined, { ...sig, local, page: PAGE, configJson: { ...PCFG, respectPrivacySignals: false } });
  await settle();
  assert.equal(pg.els['yd-status'].textContent, 'Recording is on.');
  assert.equal(pg.els['yd-button'].hidden, false);
  assert.equal(pg.els['yd-button'].textContent, 'Turn off');
  click(pg.els['yd-button']);
  assert.equal(pg.els['yd-status'].textContent, 'Recording is off. Nothing is recorded.');
}

// --- tier 2 and cohorts: only for study participants who opted in
{
  const env = boot({ ...CFG, study: true }, { search: '?cohort=eecs490-w27' });
  t(env)._test.optIn(true);
  const ev = JSON.parse(env.fetches[0]).events[0];
  assert.equal(ev.tier, 2);
  assert.equal(ev.cohort, 'eecs490-w27');
  assert.equal(ev.props.age_18_plus, true);
  const tier1 = boot(CFG, { search: '?cohort=eecs490-w27' });
  t(tier1)._test.optIn(false);
  const ev1 = JSON.parse(tier1.fetches[0]).events[0];
  assert.equal(ev1.tier, 1);
  assert.equal(ev1.cohort, undefined);
}

// --- programs: text only with collectCode, each distinct text once
{
  const off = boot({ ...CFG, consent: 'notice', surface: 'playground' });
  const id = t(off).program('fn main() {}');
  assert.equal(id, createHash('sha256').update('fn main() {}').digest('hex'));
  assert.ok(!names(off).includes('program'));
  const on = boot({ ...CFG, consent: 'notice', surface: 'playground', collectCode: true });
  t(on).program('fn main() {}', { source_figure_id: 'fd7e25f9ee13a174' });
  t(on).program('fn main() {}');
  t(on).program('fn main() { }');
  // each is sent at once (plain fetch, not a page-hide beacon)
  const progs = on.fetches.flatMap(b => JSON.parse(b).events).filter((e: any) => e.event === 'program');
  assert.equal(progs.length, 2);
  assert.equal(on.beacons.length, 0);
  assert.equal(progs[0].props.source, 'fn main() {}');
  assert.equal(progs[0].props.source_figure_id, 'fd7e25f9ee13a174');
}

// --- the opt-in bar's "what's recorded" names program text only with collectCode
{
  const off = boot({ ...CFG, consent: 'opt-in', surface: 'playground' });
  assert.doesNotMatch(off.body.children[0].text(), /including the text/);
  const on = boot({ ...CFG, consent: 'opt-in', surface: 'playground', collectCode: true });
  assert.match(on.body.children[0].text(), /including the text of the programs you run/);
}

// --- batching: <= 100 events and < 60 000 UTF-8 bytes per ordinary request
//     (the browser's 64 KiB beacon cap); a program record always alone
{
  const env = boot({ ...CFG, consent: 'notice' });
  const mk = (i: number) => ({ event: 'ui', i, pad: (i % 7 === 0 ? 'é'.repeat(2000) : 'x'.repeat(10)) });
  const events: any[] = Array.from({ length: 250 }, (_, i) => mk(i));
  events.splice(5, 0, { event: 'program', props: { source: 'ü'.repeat(30000) } });
  const out: { body: string; program: boolean }[] = JSON.parse(JSON.stringify(t(env)._test.batches(events)));
  let n = 0;
  for (const b of out) {
    const parsed = JSON.parse(b.body);
    assert.deepEqual(Object.keys(parsed), ['events']);
    assert.ok(parsed.events.length <= 100);
    const bytes = Buffer.byteLength(b.body, 'utf8');
    if (b.program) {
      assert.equal(parsed.events.length, 1);
      assert.equal(parsed.events[0].event, 'program');
      assert.ok(bytes <= 192 * 1024);
    } else {
      assert.ok(bytes < 60000, String(bytes));
      assert.ok(parsed.events.every((e: any) => e.event !== 'program'));
    }
    n += parsed.events.length;
  }
  assert.equal(n, 251);
  // oversized events are dropped, not sent (24 KiB; 80 KiB for a program)
  assert.equal(t(env)._test.batches([{ big: 'x'.repeat(30000) }]).length, 0);
  assert.equal(t(env)._test.batches([{ event: 'program', s: 'x'.repeat(90 * 1024) }]).length, 0);
}

// --- a program over the 64 KiB source cap is not sent, and is flagged
{
  const env = boot({ ...CFG, consent: 'notice', surface: 'playground', collectCode: true });
  const big = '// ' + 'é'.repeat(33 * 1024) + '\nfn main() {}';   // < 64 Ki chars but > 64 KiB of UTF-8
  const id = t(env).program(big);
  assert.equal(id, createHash('sha256').update(big).digest('hex'));
  assert.equal(t(env).programOmitted(id), true);
  assert.equal(t(env).programOmitted(t(env).program('fn main() {}')), false);
  const sent = env.fetches.flatMap(b => JSON.parse(b).events).filter((e: any) => e.event === 'program');
  assert.deepEqual(sent.map((e: any) => e.props.source), ['fn main() {}']);
}

// --- tooltip_dwell from the helpers.js hook, with the plugin's attributes
{
  const tl = new FakeEl('div', ['ex2', 'tl_panel']);
  tl.setAttribute('data-rv-figure-id', '0123456789abcdef');
  tl.appendChild(new FakeEl('svg'));
  const env = boot({ ...CFG, consent: 'notice', surface: 'playground' }, { figures: [tl] });
  const dot = new FakeEl('g', ['tooltip-trigger']);
  dot.appendChild(new FakeEl('circle'));
  // the plugin's attributes (element-ids.md)
  Object.assign(dot.attrs, { 'data-rv-id': 's@L5:dot:move', 'data-rv-kind': 'dot', 'data-rv-event': 'move static_die',
                             'data-rv-var': 's', 'data-rv-line': '5', 'data-rv-drop': 'true' });
  t(env).tooltipHover('ex2', dot, 1763370002700, 1763370003500);
  const ev = q(env).find((e: any) => e.event === 'tooltip_dwell');
  checkEnvelope(ev);
  assert.deepEqual(ev.props, {
    figure_id: '0123456789abcdef', element_id: 's@L5:dot:move', element_kind: 'dot',
    enter_ts: 1763370002700, leave_ts: 1763370003500, dwell_ms: 800,
    event_kind: 'move', var: 's', line: 5, drop: true,
  });
  // a bracket segment: kind from the plugin, state and line range
  const seg = new FakeEl('path', ['staticref', 'tooltip-trigger']);
  Object.assign(seg.attrs, { 'data-rv-id': 'r@L3-7:bracket', 'data-rv-kind': 'bracket', 'data-rv-state': 'full_privilege',
                             'data-rv-var': 'r', 'data-rv-line': '3', 'data-rv-line-end': '7' });
  t(env).tooltipHover('ex2', seg, 1763370005000, 1763370005100);
  const evb = q(env).filter((e: any) => e.event === 'tooltip_dwell').pop();
  assert.equal(evb.props.element_kind, 'bracket');
  assert.equal(evb.props.state, 'full_privilege');
  assert.equal(evb.props.line_end, 7);
  assert.equal(evb.props.event_kind, undefined);
  // a hover that never moved (no start time) is still logged, as 0 ms
  const arrow = new FakeEl('polyline', ['tooltip-trigger']);
  t(env).tooltipHover('ex2', arrow, null, 1763370004000);
  const ev2 = q(env).filter((e: any) => e.event === 'tooltip_dwell').pop();
  assert.equal(ev2.props.element_kind, 'arrow');
  assert.equal(ev2.props.dwell_ms, 0);
  assert.match(ev2.props.element_id, /^\S{1,128}$/);
  assert.equal(ev2.props.event_kind, undefined);
}

// --- playground helpers
assert.equal(TELEMETRY_ENABLED, false, 'off unless VITE_RV_TELEMETRY_ENDPOINT is set');
assert.deepEqual(errorCodes('error[E0382]: a\nerror[E0499]: b\nerror[E0382]: c\nerror: no code'), ['E0382', 'E0499']);
assert.equal(crashLocation('RustViz can\'t visualize this program yet (internal error at expr_visitor.rs:862: called'), 'expr_visitor.rs:862');
assert.equal(crashLocation('RustViz does not support async'), null);
assert.equal(classifyOutcome({ cached: true }), 'ok');
assert.equal(classifyOutcome({ status: 200 }), 'ok');
assert.equal(classifyOutcome({ network: true }), 'network_error');
assert.equal(classifyOutcome({ status: 429 }), 'rate_limited');
assert.equal(classifyOutcome({ status: 422, errorKind: 'does-not-compile' }), 'compile_error');
assert.equal(classifyOutcome({ status: 422, errorKind: 'unsupported', message: '(internal error at a.rs:1: x)' }), 'crash');
assert.equal(classifyOutcome({ status: 422, errorKind: 'unsupported', message: 'not supported' }), 'refused');
assert.equal(classifyOutcome({ status: 504, errorKind: 'timeout' }), 'timeout');
assert.equal(classifyOutcome({ status: 503, errorKind: 'sandbox-unavailable' }), 'server_error');

console.log('telemetry ok');
