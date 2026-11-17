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
  remove() {
    const p = this.parentElement;
    if (p) p.children = p.children.filter(c => c !== this);
  }
  addEventListener(t: string, fn: Listener) { (this.listeners[t] ||= []).push(fn); }
  removeEventListener() {}
  focus() {}
  closest(sel: string): FakeEl | null {
    let e: FakeEl | null = this;
    while (e) { if (sel === '.vis_block' && e.classList.contains('vis_block')) return e; e = e.parentElement; }
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
} = {}): Env {
  const beacons: string[] = [];
  const fetches: string[] = [];
  const local = opts.local ?? new Map<string, string>();
  const body = new FakeEl('body');
  const head = new FakeEl('head');
  const figures = opts.figures ?? [];
  const document = {
    readyState: 'complete',
    visibilityState: 'visible',
    referrer: '',
    body, head, documentElement: new FakeEl('html'),
    activeElement: null,
    hasFocus: () => true,
    createElement: (t: string) => new FakeEl(t),
    createTextNode: (t: string) => { const e = new FakeEl('#text'); e.textContent = t; return e; },
    getElementById: () => null,
    querySelector: (sel: string) => {
      const m = /^\.([\w-]+)\.code_panel$/.exec(sel);
      return m ? null : null;
    },
    querySelectorAll: (sel: string) => (sel === '.tl_panel' ? figures : []),
    addEventListener: () => {},
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
    fetch: (_url: string, init: { body: string }) => { fetches.push(init.body); return Promise.resolve({ status: 200 }); },
    setInterval: () => 0,
    setTimeout: () => 0,
    addEventListener: () => {},
    innerWidth: 1234, innerHeight: 777, scrollY: 0,
  };
  win.window = win;
  vm.createContext(win);
  vm.runInContext(SRC, win);
  return { win, beacons, fetches, local, body, figures };
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

const CFG = { endpoint: 'http://127.0.0.1:8090/', rvVersion: '2.1.0', surface: 'tutorial' };

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
  assert.equal(env.body.children.length, 1, 'banner shown');
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

// --- notice mode: events flow, the first one says the notice was shown
{
  const env = boot({ ...CFG, consent: 'notice' });
  assert.equal(t(env).isActive(), true);
  assert.deepEqual(names(env), ['consent', 'page_view']);
  assert.equal(q(env)[0].props.choice, 'notice_shown');
  assert.equal(q(env)[0].consent_mode, 'notice');
  q(env).forEach(checkEnvelope);
  assert.match(env.body.children[0].text(), /This site records how it is used/);
  t(env)._test.dismissNotice();
  assert.equal(q(env)[2].props.choice, 'notice_dismissed');
  const again = boot({ ...CFG, consent: 'notice' }, { local: env.local });
  assert.deepEqual(names(again), ['page_view'], 'notice_shown is sent once');
  // a site that switches to opt-in asks again
  const optin = boot(CFG, { local: env.local });
  assert.equal(t(optin).isActive(), false);
}

// --- withdraw: one last event with the ids, then nothing and no ids
{
  const env = boot({ ...CFG, consent: 'notice' });
  const anon = env.local.get('rv.telemetry.anon_id');
  assert.match(anon!, UUID);
  t(env)._test.withdraw(true);
  const sent = env.beacons.flatMap(b => JSON.parse(b).events);
  const w = sent[sent.length - 1];
  assert.equal(w.event, 'withdraw');
  assert.equal(w.anon_id, anon);
  assert.equal(w.props.delete_requested, true);
  assert.equal(env.local.get('rv.telemetry.anon_id'), undefined);
  assert.equal(env.local.get('rv.telemetry.session'), undefined);
  assert.equal(t(env).isActive(), false);
  assert.equal(t(env).emit('ui', { action: 'other' }), null);
  const again = boot({ ...CFG, consent: 'notice' }, { local: env.local });
  assert.equal(t(again).isActive(), false, 'stays off on later visits');
  assert.deepEqual(names(again), []);
}

// --- Do Not Track / Global Privacy Control: nothing, in every mode
for (const sig of [{ dnt: true }, { gpc: true }]) {
  for (const consent of ['notice', 'opt-in']) {
    const env = boot({ ...CFG, consent }, sig);
    assert.equal(t(env).isActive(), false);
    assert.deepEqual(names(env), []);
    assert.equal(env.body.children.length, 0, 'no banner');
    assert.equal(env.local.size, 0, 'nothing stored');
    t(env).openPanel();
    assert.match(env.body.children[0].text(), /records nothing and stores no ID/);
    assert.equal(env.fetches.length + env.beacons.length, 0);
  }
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
  assert.doesNotMatch(off.body.children[0].text(), /including the text/);
  const on = boot({ ...CFG, consent: 'notice', surface: 'playground', collectCode: true });
  assert.match(on.body.children[0].text(), /including the text of the programs you run/);
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

// --- batching: <= 100 events and < 64 KiB per request
{
  const env = boot({ ...CFG, consent: 'notice' });
  const batches: string[] = t(env)._test.batches(
    Array.from({ length: 250 }, (_, i) => ({ event: 'ui', i, pad: 'x'.repeat(i % 7 === 0 ? 2000 : 10) })));
  assert.ok(batches.length >= 3);
  let n = 0;
  for (const b of batches) {
    const parsed = JSON.parse(b);
    assert.deepEqual(Object.keys(parsed), ['events']);
    assert.ok(parsed.events.length <= 100);
    assert.ok(b.length < 64 * 1024);
    n += parsed.events.length;
  }
  assert.equal(n, 250);
  // an oversized event is dropped, not sent
  assert.equal(t(env)._test.batches([{ big: 'x'.repeat(30000) }]).length, 0);
}

// --- tooltip_dwell from the helpers.js hook, with the plugin's attributes
{
  const tl = new FakeEl('div', ['ex2', 'tl_panel']);
  tl.setAttribute('data-rv-figure-id', '0123456789abcdef');
  tl.appendChild(new FakeEl('svg'));
  const env = boot({ ...CFG, consent: 'notice', surface: 'playground' }, { figures: [tl] });
  const dot = new FakeEl('g', ['tooltip-trigger']);
  dot.appendChild(new FakeEl('circle'));
  Object.assign(dot.attrs, { 'data-rv-id': 's:L5:move-out', 'data-rv-kind': 'move', 'data-rv-var': 's', 'data-rv-line': '5' });
  t(env).tooltipHover('ex2', dot, 1763370002700, 1763370003500);
  const ev = q(env).find((e: any) => e.event === 'tooltip_dwell');
  checkEnvelope(ev);
  assert.deepEqual(ev.props, {
    figure_id: '0123456789abcdef', element_id: 's:L5:move-out', element_kind: 'dot',
    enter_ts: 1763370002700, leave_ts: 1763370003500, dwell_ms: 800,
    event_kind: 'move', var: 's', line: 5,
  });
  // a hover that never moved (no start time) is still logged, as 0 ms
  const arrow = new FakeEl('polyline', ['tooltip-trigger']);
  t(env).tooltipHover('ex2', arrow, null, 1763370004000);
  const ev2 = q(env).filter((e: any) => e.event === 'tooltip_dwell')[1];
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
