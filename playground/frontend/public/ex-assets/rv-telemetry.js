// rv-telemetry.js: RustViz's usage-logging client. OFF unless configured.
//
// This file is shared, byte for byte, by mdbook-rustviz (embedded from
// rustviz-lib/src/rv-telemetry.js) and the playground
// (playground/frontend/public/ex-assets/rv-telemetry.js); a corpus test
// checks the copies match. Nothing loads it unless a telemetry endpoint is
// configured (mdbook-rustviz: `telemetry-endpoint` in book.toml; the
// playground: VITE_RV_TELEMETRY_ENDPOINT at build time). Without
// `window.RV_TELEMETRY_CONFIG.endpoint` it does nothing at all.
//
// The event contract (envelope, event names, properties) is schema 1 of
// the collector, playground/events/ (see its README / schema doc).
//
// Config, read once from window.RV_TELEMETRY_CONFIG before this script runs:
//   endpoint        collector origin, events go to `${endpoint}/v1/events`
//   consent         'opt-in' (default: nothing is sent before the visitor
//                   says yes) | 'notice' (a banner says what is recorded;
//                   recording starts without a click; "Turn off" stops it)
//   collectCode     false (default) | true: send the text of programs run in
//                   the playground, once per distinct program (`program`)
//   surface         'tutorial' | 'playground' | 'book:<host>' (default
//                   'book:' + location.hostname)
//   rvVersion       RustViz version that rendered the figures
//   privacyUrl      link shown in the banner and the "Your data" panel
//   consentVersion  version string of the notice text (default below)
//   study           false (default) | true: the opt-in asks for 18+ and
//                   marks events tier 2 (research participant), which also
//                   sends the `?cohort=` tag
//   studyText       a sentence about the research study, shown in the banner
//                   and the panel (always shown with `study`; the default
//                   is an obvious placeholder for the maintainer / IRB)
//   panelLink       'inline' (default: a small "Your data" link at the end of
//                   the page's <main>) | 'none' (the page links to the panel
//                   itself, e.g. the playground's info pane, via
//                   rvTelemetry.openPanel())
//   bannerSlot      CSS selector of an element to put the consent bar in
//                   (default: the top of <main>, else of <body>). The bar is
//                   in the page's flow, never on top of the content.
//
// Do Not Track or Global Privacy Control turns everything off: no banner,
// no ids, no events, whatever the config says.
(function () {
    'use strict';
    if (typeof window === 'undefined' || window.rvTelemetry) return;
    var userCfg = window.RV_TELEMETRY_CONFIG;
    if (!userCfg || typeof userCfg.endpoint !== 'string' || !userCfg.endpoint) return;

    var CLIENT_VERSION = '0.1.0';
    var SCHEMA = 1;
    var FLUSH_MS = 10000;
    var SESSION_IDLE_MS = 30 * 60 * 1000;
    var ACTIVE_INPUT_MS = 30000;
    var SAMPLE_MS = 100;                 // pointer_trace: 10 Hz
    var MAX_SAMPLES = 600;               // per pointer_trace chunk
    var MAX_TAPS = 100;
    var MAX_QUEUE = 1000;                // events waiting to be sent
    var MAX_BATCH_EVENTS = 100;
    // Sizes are UTF-8 bytes. The collector accepts request bodies up to
    // 192 KiB, but a beacon / keepalive request is capped by the browser at
    // 64 KiB in flight, so ordinary batches stay under 60 000 bytes and a
    // `program` record always travels alone in a plain fetch.
    var MAX_BATCH_BYTES = 60000;
    var MAX_EVENT_BYTES = 24 * 1024;     // collector: larger events are rejected ...
    var MAX_PROGRAM_BYTES = 80 * 1024;   // ... except a `program`, up to 80 KiB
    var MAX_SOURCE_BYTES = 64 * 1024;    // collector's cap on `program.source`
    var MAX_BODY_BYTES = 192 * 1024;     // collector's cap on a request body
    var KEY = 'rv.telemetry.';           // localStorage / sessionStorage prefix

    var cfg = {
        endpoint: userCfg.endpoint.replace(/\/+$/, ''),
        consent: userCfg.consent === 'notice' ? 'notice' : 'opt_in',
        collectCode: userCfg.collectCode === true,
        surface: validSurface(userCfg.surface) || ('book:' + hostToken(location.hostname)),
        rvVersion: token32(userCfg.rvVersion) || 'unknown',
        privacyUrl: typeof userCfg.privacyUrl === 'string' ? userCfg.privacyUrl : '',
        consentVersion: token32(userCfg.consentVersion) || '2026-11',
        study: userCfg.study === true,
        studyText: typeof userCfg.studyText === 'string' ? userCfg.studyText
            : '[Placeholder for the research-study sentence: study name, who runs it ' +
              '(PI) and a contact address. To be written by the maintainer / IRB.]',
        panelLink: userCfg.panelLink === 'none' ? 'none' : 'inline',
        bannerSlot: typeof userCfg.bannerSlot === 'string' ? userCfg.bannerSlot : '',
    };
    var showStudy = cfg.study || typeof userCfg.studyText === 'string';

    // ------------------------------------------------------------------
    // Small utilities
    // ------------------------------------------------------------------

    function token32(s) {
        return (typeof s === 'string' && /^[0-9A-Za-z.+-]{1,32}$/.test(s)) ? s : null;
    }
    function hostToken(h) {
        h = String(h || 'local').toLowerCase().replace(/[^a-z0-9.-]/g, '').slice(0, 64);
        return h || 'local';
    }
    function validSurface(s) {
        return (s === 'tutorial' || s === 'playground' ||
                (typeof s === 'string' && /^book:[a-z0-9.-]{1,64}$/.test(s))) ? s : null;
    }
    function now() { return Date.now(); }

    function store(kind) {
        try { return window[kind]; } catch (e) { return null; }
    }
    function getItem(kind, k) {
        try { var s = store(kind); return s ? s.getItem(KEY + k) : null; } catch (e) { return null; }
    }
    function setItem(kind, k, v) {
        try { var s = store(kind); if (s) s.setItem(KEY + k, v); } catch (e) { /* storage off */ }
    }
    function removeItem(kind, k) {
        try { var s = store(kind); if (s) s.removeItem(KEY + k); } catch (e) { /* storage off */ }
    }

    function uuid() {
        var c = window.crypto;
        if (c && typeof c.randomUUID === 'function') return c.randomUUID();
        var b = new Uint8Array(16);
        if (c && c.getRandomValues) c.getRandomValues(b);
        else for (var i = 0; i < 16; i++) b[i] = Math.floor(Math.random() * 256);
        b[6] = (b[6] & 0x0f) | 0x40;
        b[8] = (b[8] & 0x3f) | 0x80;
        var h = '';
        for (var j = 0; j < 16; j++) h += (b[j] + 0x100).toString(16).slice(1);
        return h.slice(0, 8) + '-' + h.slice(8, 12) + '-' + h.slice(12, 16) + '-' +
               h.slice(16, 20) + '-' + h.slice(20);
    }
    var UUID_RE = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;

    // SHA-256 of a string's UTF-8 bytes, as lowercase hex. Synchronous (so
    // ids are ready when an event is created) and works outside secure
    // contexts, unlike crypto.subtle.
    var K256 = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
        0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
        0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
        0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2
    ];
    function sha256(str) {
        var bytes = new TextEncoder().encode(str);
        var len = bytes.length;
        var nBlocks = ((len + 9 + 63) >> 6);
        var m = new Uint8Array(nBlocks * 64);
        m.set(bytes);
        m[len] = 0x80;
        var bits = len * 8;
        var dv = new DataView(m.buffer);
        dv.setUint32(m.length - 4, bits >>> 0);
        dv.setUint32(m.length - 8, Math.floor(bits / 0x100000000));
        var H = [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
        var w = new Array(64);
        for (var off = 0; off < m.length; off += 64) {
            for (var i = 0; i < 16; i++) w[i] = dv.getUint32(off + i * 4);
            for (i = 16; i < 64; i++) {
                var x = w[i - 15], y = w[i - 2];
                var s0 = ((x >>> 7) | (x << 25)) ^ ((x >>> 18) | (x << 14)) ^ (x >>> 3);
                var s1 = ((y >>> 17) | (y << 15)) ^ ((y >>> 19) | (y << 13)) ^ (y >>> 10);
                w[i] = (w[i - 16] + s0 + w[i - 7] + s1) | 0;
            }
            var a = H[0], b = H[1], c = H[2], d = H[3], e = H[4], f = H[5], g = H[6], h = H[7];
            for (i = 0; i < 64; i++) {
                var S1 = ((e >>> 6) | (e << 26)) ^ ((e >>> 11) | (e << 21)) ^ ((e >>> 25) | (e << 7));
                var t1 = (h + S1 + ((e & f) ^ (~e & g)) + K256[i] + w[i]) | 0;
                var S0 = ((a >>> 2) | (a << 30)) ^ ((a >>> 13) | (a << 19)) ^ ((a >>> 22) | (a << 10));
                var t2 = (S0 + ((a & b) ^ (a & c) ^ (b & c))) | 0;
                h = g; g = f; f = e; e = (d + t1) | 0; d = c; c = b; b = a; a = (t1 + t2) | 0;
            }
            H[0] = (H[0] + a) | 0; H[1] = (H[1] + b) | 0; H[2] = (H[2] + c) | 0; H[3] = (H[3] + d) | 0;
            H[4] = (H[4] + e) | 0; H[5] = (H[5] + f) | 0; H[6] = (H[6] + g) | 0; H[7] = (H[7] + h) | 0;
        }
        var out = '';
        for (i = 0; i < 8; i++) out += ((H[i] >>> 0) + 0x100000000).toString(16).slice(1);
        return out;
    }

    // figure_id: first 16 hex of SHA-256(snippet + "\n" + rv_version), the
    // same formula mdbook-rustviz uses at build time.
    function figureId(code) { return sha256(code + '\n' + cfg.rvVersion).slice(0, 16); }
    // program_id: SHA-256 of the exact source text.
    function programId(code) { return sha256(code); }

    // ------------------------------------------------------------------
    // Privacy signals and consent state
    // ------------------------------------------------------------------

    function privacySignal() {
        var n = window.navigator || {};
        return n.globalPrivacyControl === true ||
            n.doNotTrack === '1' || n.doNotTrack === 'yes' ||
            window.doNotTrack === '1' || n.msDoNotTrack === '1';
    }
    var blocked = privacySignal();

    // Stored choice: {state: 'on' | 'off', how: 'opt_in' | 'notice' |
    // 'declined' | 'withdrawn', version, age18?}. No identifier in it.
    function loadChoice() {
        try { return JSON.parse(getItem('localStorage', 'choice') || 'null'); } catch (e) { return null; }
    }
    function saveChoice(c) { setItem('localStorage', 'choice', JSON.stringify(c)); }

    function initiallyOn(choice) {
        if (blocked) return false;
        if (!choice) return cfg.consent === 'notice';
        if (choice.state !== 'on') return false;
        // A notice-mode "on" isn't an opt-in: an opt-in site asks again.
        return cfg.consent === 'notice' || choice.how === 'opt_in';
    }

    var choice = loadChoice();
    var active = initiallyOn(choice);

    // ------------------------------------------------------------------
    // Ids and the envelope
    // ------------------------------------------------------------------

    var pageLoadId = uuid();
    var seq = 0;
    var cohort = (function () {
        var m = /[?&]cohort=([a-z0-9-]{1,40})(?:&|$)/.exec(location.search || '');
        if (m) { setItem('sessionStorage', 'cohort', m[1]); return m[1]; }
        var s = getItem('sessionStorage', 'cohort');
        return (s && /^[a-z0-9-]{1,40}$/.test(s)) ? s : null;
    })();

    function anonId(create) {
        var id = getItem('localStorage', 'anon_id');
        if (id && UUID_RE.test(id)) return id;
        if (!create) return null;
        id = uuid();
        setItem('localStorage', 'anon_id', id);
        return id;
    }
    function sessionId() {
        var t = now(), s = null;
        try { s = JSON.parse(getItem('localStorage', 'session') || 'null'); } catch (e) { s = null; }
        if (!s || !UUID_RE.test(s.id) || !(t - s.last < SESSION_IDLE_MS)) s = { id: uuid(), last: t };
        s.last = t;
        setItem('localStorage', 'session', JSON.stringify(s));
        return s.id;
    }
    function tier() {
        return (cfg.study && choice && choice.how === 'opt_in' && choice.age18) ? 2 : 1;
    }
    function referrerHost() {
        try {
            if (!document.referrer) return null;
            var h = new URL(document.referrer).hostname.toLowerCase();
            if (!h || h === location.hostname.toLowerCase()) return null;
            return /^[a-z0-9.-]{1,253}$/.test(h) ? h : null;
        } catch (e) { return null; }
    }
    var REFERRER = referrerHost();
    function bucket(n) { return Math.max(0, Math.round((n || 0) / 100) * 100); }

    function envelope(name, props) {
        var ev = {
            schema: SCHEMA,
            event_id: uuid(),
            event: name,
            seq: seq++,
            client_ts: now(),
            surface: cfg.surface,
            page: (location.pathname || '/').slice(0, 256),
            page_load_id: pageLoadId,
            session_id: sessionId(),
            anon_id: anonId(true),
            consent_mode: cfg.consent,
            tier: tier(),
            rv_version: cfg.rvVersion,
            client_version: CLIENT_VERSION,
            viewport: bucket(window.innerWidth) + 'x' + bucket(window.innerHeight),
            tz_offset: -new Date().getTimezoneOffset(),
            props: props || {}
        };
        var lang = (navigator.language || '').replace(/[^A-Za-z0-9-]/g, '').slice(0, 35);
        if (lang) ev.lang = lang;
        if (REFERRER) ev.referrer_host = REFERRER;
        if (ev.tier === 2 && cohort) ev.cohort = cohort;
        return ev;
    }

    // ------------------------------------------------------------------
    // Queue and transport
    // ------------------------------------------------------------------

    var queue = [];
    var retryOnce = [];      // a batch that failed with a network error/5xx/429
    var beforeFlush = [];    // callbacks that turn open state into events

    function emit(name, props) {
        if (!active) return null;
        var ev = envelope(name, props);
        if (queue.length >= MAX_QUEUE) return null;    // dropped; seq shows the gap
        queue.push(ev);
        return ev;
    }

    function utf8Bytes(s) { return new TextEncoder().encode(s).length; }

    // Request bodies for `events`: {body, program}. Each `program` record
    // is a body of its own; the rest are packed into bodies of at most 100
    // events and MAX_BATCH_BYTES. An event the collector would reject for
    // its size is dropped here.
    function batches(events) {
        var out = [], cur = [], size = 12;
        function wrap(list) { return '{"events":[' + list.join(',') + ']}'; }
        for (var i = 0; i < events.length; i++) {
            var s = JSON.stringify(events[i]);
            var n = utf8Bytes(s);
            if (events[i].event === 'program') {
                if (n <= MAX_PROGRAM_BYTES && n + 12 <= MAX_BODY_BYTES) out.push({ body: wrap([s]), program: true });
                continue;
            }
            if (n > MAX_EVENT_BYTES) continue;
            if (cur.length && (cur.length >= MAX_BATCH_EVENTS || size + n + 1 > MAX_BATCH_BYTES)) {
                out.push({ body: wrap(cur), program: false }); cur = []; size = 12;
            }
            cur.push(s); size += n + 1;
        }
        if (cur.length) out.push({ body: wrap(cur), program: false });
        return out;
    }

    // `useBeacon` only for bodies under the 64 KiB beacon/keepalive cap
    // (every ordinary batch); a program record goes in a plain fetch.
    function send(batch, useBeacon, isRetry) {
        var body = batch.body;
        var url = cfg.endpoint + '/v1/events';
        if (useBeacon && navigator.sendBeacon) {
            try {
                // A string body goes as text/plain: no CORS preflight.
                if (navigator.sendBeacon(url, body)) return;
            } catch (e) { /* fall through to fetch */ }
        }
        if (typeof fetch !== 'function') return;
        try {
            fetch(url, {
                // keepalive requests are capped at 64 KiB in flight
                method: 'POST', body: body, keepalive: useBeacon, credentials: 'omit', mode: 'cors',
                headers: { 'Content-Type': 'text/plain;charset=UTF-8' }
            }).then(function (r) {
                if ((r.status >= 500 || r.status === 429) && !isRetry) retryOnce.push(batch);
            }, function () {
                if (!isRetry) retryOnce.push(batch);
            });
        } catch (e) { /* dropped */ }
    }

    function flush(useBeacon) {
        for (var i = 0; i < beforeFlush.length; i++) {
            try { beforeFlush[i](); } catch (e) { /* never break the page */ }
        }
        var retries = retryOnce.splice(0, 2);
        retryOnce.length = 0;   // at most two retried batches per flush; older ones dropped
        for (i = 0; i < retries.length; i++) send(retries[i], useBeacon && !retries[i].program, true);
        if (!queue.length) return;
        var bodies = batches(queue.splice(0, queue.length));
        for (i = 0; i < bodies.length; i++) send(bodies[i], useBeacon && !bodies[i].program, false);
    }

    // ------------------------------------------------------------------
    // Page activity: page_view / page_hide
    // ------------------------------------------------------------------

    var loadTs = now();
    var activeMs = 0, lastTick = now(), lastInput = now(), maxScroll = 0;
    var pageViewSent = false, lastHideTs = 0;

    function markInput() { lastInput = now(); }
    function isActive() {
        return document.visibilityState === 'visible' &&
            (typeof document.hasFocus !== 'function' || document.hasFocus()) &&
            now() - lastInput < ACTIVE_INPUT_MS;
    }
    function tick() {
        var t = now();
        if (isActive()) activeMs += Math.min(t - lastTick, 2000);
        lastTick = t;
    }
    function scrollPct() {
        var de = document.documentElement, b = document.body;
        var h = Math.max(de ? de.scrollHeight : 0, b ? b.scrollHeight : 0);
        if (!h) return 0;
        var seen = (window.scrollY || 0) + (window.innerHeight || 0);
        return Math.max(0, Math.min(100, Math.round(seen / h * 100)));
    }
    function noteScroll() { maxScroll = Math.max(maxScroll, scrollPct()); markInput(); }

    function navType() {
        try {
            var e = performance.getEntriesByType('navigation')[0];
            var t = e && e.type;
            return (t === 'navigate' || t === 'reload' || t === 'back_forward' || t === 'prerender') ? t : null;
        } catch (e2) { return null; }
    }
    function sendPageView() {
        if (pageViewSent) return;
        pageViewSent = true;
        var t = navType();
        emit('page_view', t ? { nav_type: t } : {});
    }
    function pageHide(reason) {
        if (!active) return;
        var t = now();
        // pagehide and visibilitychange both fire when leaving a page, in
        // either order: one page_hide for the pair
        if (t - lastHideTs < 1000) { flush(true); return; }
        lastHideTs = t;
        tick();
        maxScroll = Math.max(maxScroll, scrollPct());
        closeAllFigures(t);
        emit('page_hide', {
            active_ms: Math.min(activeMs, 86400000), total_ms: Math.min(t - loadTs, 86400000),
            max_scroll_pct: maxScroll, reason: reason
        });
        flush(true);
    }

    // ------------------------------------------------------------------
    // Figures: figure_view, tooltip_dwell, code_line_hover, pointer_trace
    // ------------------------------------------------------------------

    // A figure is a code panel + timeline panel sharing a class
    // (`example-N` in mdbook, `ex2` in the playground). Its id comes from
    // `data-rv-figure-id` on the timeline panel or an ancestor (written by
    // mdbook-rustviz / the playground), else from the code panel's text.
    var figures = [];   // {cls, tl, code, box, id, index, ...}

    function panelSvg(panel) {
        if (!panel) return null;
        try {
            if (panel.contentDocument) return panel.contentDocument.documentElement;
        } catch (e) { return null; }
        if (panel.tagName && panel.tagName.toLowerCase() === 'svg') return panel;
        return panel.querySelector ? panel.querySelector('svg') : null;
    }
    function figureClass(tl) {
        var cl = tl.classList;
        for (var i = 0; i < cl.length; i++) if (cl[i] !== 'tl_panel' && cl[i] !== 'code_panel') return cl[i];
        return null;
    }
    function figureIdOf(f) {
        var el = f.tl;
        while (el && el.getAttribute) {
            var v = el.getAttribute('data-rv-figure-id');
            if (v && /^[0-9a-f]{16,64}$/.test(v)) return v;
            el = el.parentElement;
        }
        var svg = panelSvg(f.code);
        var code = svg && svg.querySelector('#code');
        return code ? figureId('code-panel:' + code.textContent) : null;
    }
    function findFigure(cls) {
        for (var i = 0; i < figures.length; i++) if (figures[i].cls === cls) return figures[i];
        return null;
    }
    // The figure's current id (the playground replaces its figure in place).
    function fid(f) {
        var id = figureIdOf(f);
        if (id && id !== f.id) {
            if (f.id) {
                // a new figure in the same place: close the old one's intervals
                var t = now();
                closeFigureView(f, t);
                endCodeLine(f, t);
                closeChunk(f);
                if (f.intersecting && document.visibilityState === 'visible') f.visibleSince = t;
            }
            f.id = id;
            f.chunk = 0;
        }
        return f.id;
    }

    function scan() {
        // a figure taken off the page (the playground shows an error card
        // instead) stops being viewed
        for (var j = 0; j < figures.length; j++) {
            if (figures[j].tl.isConnected === false) {
                closeFigureView(figures[j], now());
                figures[j].intersecting = false;
            }
        }
        var tls = document.querySelectorAll('.tl_panel');
        for (var i = 0; i < tls.length; i++) {
            var tl = tls[i], cls = figureClass(tl);
            if (!cls) continue;
            var f = findFigure(cls);
            if (f && f.tl === tl) { fid(f); attachPanels(f); continue; }
            if (f) { detach(f); f.tl = tl; }
            else {
                f = { cls: cls, tl: tl, index: figures.length, visibleSince: null, id: null,
                      chunk: 0, trace: null, line: null, docs: [] };
                figures.push(f);
            }
            f.code = document.querySelector('.' + cls + '.code_panel');
            f.box = tl.closest ? (tl.closest('.vis_block') || tl.parentElement || tl) : tl;
            f.id = figureIdOf(f);
            observe(f);
            attachPanels(f);
        }
    }

    // figure_view: one event per interval with >= 50 % of the figure visible.
    var io = null;
    function observe(f) {
        if (typeof IntersectionObserver !== 'function') return;
        if (!io) {
            io = new IntersectionObserver(function (entries) {
                var t = now();
                for (var i = 0; i < entries.length; i++) {
                    var f = boxFigure(entries[i].target);
                    if (!f) continue;
                    f.intersecting = entries[i].intersectionRatio >= 0.5;
                    if (f.intersecting && document.visibilityState === 'visible') {
                        if (f.visibleSince === null) f.visibleSince = t;
                    } else closeFigureView(f, t);
                }
            }, { threshold: [0, 0.5, 1] });
        }
        if (f.observed === f.box) return;
        if (f.observed) io.unobserve(f.observed);
        io.observe(f.box);
        f.observed = f.box;
    }
    function boxFigure(box) {
        for (var i = 0; i < figures.length; i++) if (figures[i].observed === box) return figures[i];
        return null;
    }
    function closeFigureView(f, t) {
        if (f.visibleSince === null) return;
        var start = f.visibleSince;
        f.visibleSince = null;
        if (!f.id || t <= start) return;
        emit('figure_view', {
            figure_id: f.id, figure_index: Math.min(f.index, 999),
            enter_ts: start, leave_ts: t, visible_ms: Math.min(t - start, 86400000)
        });
    }
    function closeAllFigures(t) {
        for (var i = 0; i < figures.length; i++) {
            closeFigureView(figures[i], t);
            endCodeLine(figures[i], t);
            closeChunk(figures[i]);
        }
    }
    function reopenVisibleFigures() {
        var t = now();
        for (var i = 0; i < figures.length; i++) {
            if (figures[i].intersecting && figures[i].visibleSince === null) figures[i].visibleSince = t;
        }
    }

    // Listeners inside the panels. With <object> embedding (mdbook) the
    // SVG is its own document and its events don't reach this page, so we
    // listen on the SVG roots themselves (once each document has loaded).
    function attachPanels(f) {
        listenWhenLoaded(f, f.tl, attachTimeline);
        listenWhenLoaded(f, f.code, attachCode);
    }
    function listenWhenLoaded(f, panel, attach) {
        if (!panel) return;
        var svg = panelSvg(panel);
        if (svg && svg.tagName && svg.tagName.toLowerCase() === 'svg') { attach(f, svg); return; }
        if (panel.tagName && panel.tagName.toLowerCase() === 'object' && !panel.__rvTlmWait) {
            panel.__rvTlmWait = true;
            panel.addEventListener('load', function () {
                panel.__rvTlmWait = false;
                var s = panelSvg(panel);
                if (s) attach(f, s);
            });
        }
    }
    function on(f, el, type, fn) {
        el.addEventListener(type, fn, { passive: true });
        f.docs.push([el, type, fn]);
    }
    function detach(f) {
        closeFigureView(f, now());
        endCodeLine(f, now());
        closeChunk(f);
        for (var i = 0; i < f.docs.length; i++) {
            var d = f.docs[i];
            d[0].removeEventListener(d[1], d[2]);
            d[0].__rvTlm = false;
        }
        f.docs = [];
    }

    function svgPoint(svg, e) {
        try {
            var m = svg.getScreenCTM();
            if (!m) return null;
            var p = svg.createSVGPoint();
            p.x = e.clientX; p.y = e.clientY;
            p = p.matrixTransform(m.inverse());
            if (!isFinite(p.x) || !isFinite(p.y)) return null;
            return [Math.round(p.x * 10) / 10, Math.round(p.y * 10) / 10];
        } catch (err) { return null; }
    }

    // pointer_trace: pointer positions over the timeline panel, in the
    // SVG's own coordinates, at most one sample per 100 ms.
    function attachTimeline(f, svg) {
        if (svg.__rvTlm) return;
        svg.__rvTlm = true;
        var input = function () { markInput(); };
        on(f, svg, 'pointermove', function (e) { input(); sample(f, svg, e, false); });
        on(f, svg, 'pointerdown', function (e) {
            input();
            if (e.pointerType === 'touch' || e.pointerType === 'pen') sample(f, svg, e, true);
        });
        on(f, svg, 'pointerleave', function (e) { sample(f, svg, e, false, true); });
        on(f, svg, 'keydown', input);
        on(f, svg, 'wheel', input);
    }
    function sample(f, svg, e, tap, leaving) {
        if (!active || !fid(f)) return;
        var t = now();
        var kind = (e.pointerType === 'touch' || e.pointerType === 'pen') ? e.pointerType : 'mouse';
        var tr = f.trace;
        if (tr && (tr.input !== kind || t - tr.t0 > 600000)) { closeChunk(f); tr = null; }
        if (!tap && !leaving && tr && t - tr.last < SAMPLE_MS) return;
        var p = svgPoint(svg, e);
        if (!p) return;
        if (!tr) tr = f.trace = { t0: t, input: kind, samples: [], taps: [], last: 0 };
        if (tap) { if (tr.taps.length < MAX_TAPS) tr.taps.push([t - tr.t0, p[0], p[1]]); }
        else { tr.samples.push([t - tr.t0, p[0], p[1]]); tr.last = t; }
        if (tr.samples.length >= MAX_SAMPLES || tr.taps.length >= MAX_TAPS) closeChunk(f);
    }
    function closeChunk(f) {
        var tr = f.trace;
        f.trace = null;
        if (!tr || (!tr.samples.length && !tr.taps.length) || !f.id) return;
        var props = { figure_id: f.id, chunk: f.chunk++, t0: tr.t0, input: tr.input, samples: tr.samples };
        if (tr.taps.length) props.taps = tr.taps;
        emit('pointer_trace', props);
    }

    // code_line_hover: which source line of the code panel the pointer is on.
    function attachCode(f, svg) {
        if (svg.__rvTlm) return;
        svg.__rvTlm = true;
        on(f, svg, 'pointermove', function (e) {
            markInput();
            if (!active || !fid(f)) return;
            var p = svgPoint(svg, e);
            var line = p ? lineAt(svg, p[1]) : null;
            if (f.line && f.line.line === line) return;
            endCodeLine(f, now());
            if (line) f.line = { line: line, enter: now() };
        });
        on(f, svg, 'pointerleave', function () { endCodeLine(f, now()); });
    }
    function lineAt(svg, y) {
        var g = svg.querySelector('#code');
        if (!g) return null;
        var rows = g.querySelectorAll('text');
        for (var i = 0; i < rows.length; i++) {
            var ty = parseFloat(rows[i].getAttribute('y'));
            // a row is 30 units tall with its text's baseline 20 units down
            if (y >= ty - 20 && y < ty + 10) {
                var num = rows[i].querySelector('.line-number');
                var n = num ? parseInt(num.textContent, 10) : NaN;
                return (n >= 1 && n <= 100000) ? n : null;
            }
        }
        return null;
    }
    function endCodeLine(f, t) {
        var l = f.line;
        f.line = null;
        if (!l || !f.id) return;
        emit('code_line_hover', {
            figure_id: f.id, line: l.line, enter_ts: l.enter, leave_ts: Math.max(t, l.enter),
            dwell_ms: Math.min(Math.max(t - l.enter, 0), 86400000)
        });
    }

    // tooltip_dwell: called by helpers.js when the pointer leaves a
    // tooltip trigger (helpers.js knows when the tooltip was shown).
    function elementKind(el) {
        // the plugin's data-rv-kind (element-ids.md), else guessed from the tag
        var k = el.getAttribute('data-rv-kind');
        if (k && /^(header|dot|call|fn|arrow|line|branch_line|stop_bar|bracket|code_line|other)$/.test(k)) return k;
        var tag = (el.tagName || '').toLowerCase();
        var cls = el.classList || { contains: function () { return false; } };
        if (tag === 'circle' || tag === 'use') return 'dot';
        if (tag === 'polyline') return 'arrow';
        if (tag === 'text') return 'header';
        if (tag === 'line') return 'line';
        if (tag === 'path') return (cls.contains('staticref') || cls.contains('mutref')) ? 'bracket' : 'line';
        if (tag === 'g') {
            if (el.querySelector('circle')) return 'dot';
            if (el.querySelector('polyline') || el.querySelector('polygon')) return 'arrow';
        }
        return 'other';
    }
    // Without the plugin's data-rv-id, name the element by its position
    // among the figure's triggers: stable for one figure at one rv_version.
    function fallbackElementId(el) {
        var root = el.ownerSVGElement || el.ownerDocument;
        var all = root ? root.querySelectorAll('.tooltip-trigger') : [];
        for (var i = 0; i < all.length; i++) if (all[i] === el) return 'trigger:' + i;
        return 'trigger:unknown';
    }
    function tooltipHover(cls, el, start, end) {
        if (!active || !el || !el.getAttribute) return;
        var f = findFigure(cls);
        if (!f) { scan(); f = findFigure(cls); }
        if (!f || !fid(f)) return;
        end = typeof end === 'number' ? end : now();
        start = typeof start === 'number' && start <= end ? start : end;
        var id = el.getAttribute('data-rv-id');
        if (!id || !/^\S{1,128}$/.test(id)) id = fallbackElementId(el);
        var props = {
            figure_id: f.id, element_id: id, element_kind: elementKind(el),
            enter_ts: start, leave_ts: end, dwell_ms: Math.min(end - start, 86400000)
        };
        // event_kind: the first of the events drawn there (data-rv-event)
        var kind = (el.getAttribute('data-rv-event') || '').split(' ')[0];
        if (kind && /^[a-z0-9_]{1,32}$/.test(kind)) props.event_kind = kind;
        var v = el.getAttribute('data-rv-var');
        if (v) props['var'] = v.slice(0, 64);
        var line = parseInt(el.getAttribute('data-rv-line'), 10);
        if (line >= 1 && line <= 100000) props.line = line;
        var lineEnd = parseInt(el.getAttribute('data-rv-line-end'), 10);
        if (lineEnd >= 1 && lineEnd <= 100000) props.line_end = lineEnd;
        var state = el.getAttribute('data-rv-state');
        if (state && /^[a-z0-9_]{1,32}$/.test(state)) props.state = state;
        if (el.getAttribute('data-rv-drop') === 'true') props.drop = true;
        emit('tooltip_dwell', props);
    }

    // "Open in Playground" links under mdbook figures.
    function onClick(e) {
        var a = e.target && e.target.closest ? e.target.closest('a.rv-playground-link') : null;
        if (!a || !active) return;
        var row = a.closest('.rv-playground-row') || a;
        var box = row.previousElementSibling;
        while (box && !(box.classList && box.classList.contains('vis_block'))) box = box.previousElementSibling;
        var tl = box && box.querySelector('.tl_panel');
        var f = tl ? findFigure(figureClass(tl)) : null;
        var props = { action: 'open_in_playground' };
        if (f && fid(f)) props.source_figure_id = f.id;
        emit('ui', props);
        flush(true);
    }

    // ------------------------------------------------------------------
    // Programs (playground): each distinct source once per page load
    // ------------------------------------------------------------------

    var sentPrograms = {}, omittedPrograms = {};
    // Send `source` as a `program` record (once per page load, only with
    // collectCode) and return its program_id. A source over the collector's
    // 64 KiB cap is not sent (truncating it would break program_id =
    // SHA-256(source)); programOmitted(id) then says so, and the playground
    // marks its run/edit `program_omitted: "too_large"`.
    function program(source, meta) {
        if (typeof source !== 'string') return null;
        var id = programId(source);
        if (active && cfg.collectCode && !sentPrograms[id] && utf8Bytes(source) > MAX_SOURCE_BYTES) {
            omittedPrograms[id] = true;
        } else if (active && cfg.collectCode && !sentPrograms[id]) {
            sentPrograms[id] = true;
            var props = { program_id: id, source: source };
            if (meta && /^[0-9a-f]{16,64}$/.test(meta.source_figure_id || '')) {
                props.source_figure_id = meta.source_figure_id;
            }
            emit('program', props);
            // Send it now with a plain fetch: a page-hide beacon is capped
            // at 64 KiB in flight and shouldn't carry program text.
            flush(false);
        }
        return id;
    }

    // ------------------------------------------------------------------
    // Consent UI: banner and "Your data" panel
    // ------------------------------------------------------------------

    // Colours follow the host page's theme: the playground's and mdBook's
    // own CSS variables (--bg, --fg, --bg-panel / --quote-bg, --border /
    // --quote-border, --link / --links), so a theme switch restyles the bar
    // and panel too. Elsewhere, light or dark by prefers-color-scheme.
    var CSS =
        '.rv-tlm{--rvt-bg:#fff;--rvt-fg:#1d1d1f;--rvt-border:#8a8a8a;--rvt-link:#0b57d0;' +
        'font:14px/1.45 system-ui,-apple-system,"Segoe UI",sans-serif;box-sizing:border-box;' +
        'color:var(--fg,var(--rvt-fg));background:var(--bg-panel,var(--quote-bg,var(--rvt-bg)));' +
        'border:1px solid var(--border,var(--quote-border,var(--rvt-border)))}' +
        '@media (prefers-color-scheme: dark){.rv-tlm{--rvt-bg:#1f2328;--rvt-fg:#e6e6e6;--rvt-border:#6e7681;--rvt-link:#79b8ff}}' +
        '.rv-tlm *{box-sizing:border-box}' +
        '.rv-tlm a,.rv-tlm-link{color:var(--link,var(--links,var(--rvt-link,#0b57d0)))}' +
        // the notice: a slim bar in the page's flow (it takes its own space,
        // so it never covers a diagram)
        '.rv-tlm-bar{display:block;width:100%;padding:6px 12px;border-width:0 0 1px 0;font-size:13px;margin:0}' +
        '.rv-tlm-bar .rv-tlm-line{display:flex;flex-wrap:wrap;align-items:center;gap:4px 10px}' +
        '.rv-tlm-bar .rv-tlm-text{flex:1 1 320px;min-width:0}' +
        '.rv-tlm-bar .rv-tlm-actions{display:flex;flex-wrap:wrap;gap:6px;align-items:center}' +
        '.rv-tlm-bar ul{margin:6px 0 2px;padding-left:20px}' +
        '.rv-tlm-bar.rv-tlm-boxed{border-width:1px;border-radius:6px;margin:0 0 1em}' +
        '.rv-tlm-panel{position:fixed;right:12px;bottom:12px;z-index:2147483000;width:min(420px,calc(100vw - 24px));' +
        'max-height:calc(100vh - 24px);overflow:auto;padding:16px;border-radius:8px;box-shadow:0 4px 18px rgba(0,0,0,.3)}' +
        '.rv-tlm h2{font-size:16px;margin:0 0 8px}' +
        '.rv-tlm p,.rv-tlm-panel ul{margin:0 0 8px}.rv-tlm-panel ul{padding-left:20px}' +
        '.rv-tlm code{font-size:12px;word-break:break-all}' +
        '.rv-tlm-row{display:flex;flex-wrap:wrap;gap:8px;margin-top:10px;align-items:center}' +
        '.rv-tlm button{font:inherit;padding:3px 12px;border-radius:5px;cursor:pointer;' +
        'border:1px solid var(--border-strong,var(--fg,var(--rvt-border)));' +
        'background:var(--bg,var(--rvt-bg));color:var(--fg,var(--rvt-fg))}' +
        '.rv-tlm-panel button{padding:6px 14px;min-width:7em}' +
        '.rv-tlm button:hover{filter:brightness(.93)}' +
        // link-styled buttons look like the page's own links
        '.rv-tlm button.rv-tlm-plain,.rv-tlm-link{background:none;border:none;padding:0;min-width:0;' +
        'font:inherit;color:var(--link,var(--links,var(--rvt-link,#0b57d0)));text-decoration:inherit;cursor:pointer}' +
        '.rv-tlm button.rv-tlm-plain:hover,.rv-tlm-link:hover{text-decoration:underline;filter:none}' +
        '.rv-tlm button:focus-visible,.rv-tlm-link:focus-visible{outline:2px solid currentColor;outline-offset:2px}' +
        '.rv-tlm-close{float:right;min-width:0!important;padding:2px 8px!important}' +
        '.rv-tlm-study{font-style:italic}' +
        '.rv-tlm-foot{font-size:.85em;opacity:.8;margin-top:2em}';

    function el(tag, attrs, children) {
        var e = document.createElement(tag);
        for (var k in attrs) if (Object.prototype.hasOwnProperty.call(attrs, k)) {
            if (k === 'text') e.textContent = attrs[k];
            else if (k === 'onclick') e.addEventListener('click', attrs[k]);
            else e.setAttribute(k, attrs[k]);
        }
        (children || []).forEach(function (c) { if (c) e.appendChild(typeof c === 'string' ? document.createTextNode(c) : c); });
        return e;
    }
    function ensureCss() {
        if (document.getElementById('rv-tlm-css')) return;
        var s = el('style', { id: 'rv-tlm-css' });
        s.textContent = CSS;
        (document.head || document.documentElement).appendChild(s);
    }

    // What is recorded, in plain words. Used by the banner and the panel.
    function whatList() {
        var items = [
            'which pages you open, how long they are in front of you and how far you scroll;',
            'which RustViz diagrams you look at, which parts of them you point at, for how long, ' +
            'and the path of your pointer over a diagram;'
        ];
        if (cfg.surface === 'playground') {
            items.push('in the playground: examples you pick, how much you edit, and each run’s ' +
                'result (for example the rustc error codes)' +
                (cfg.collectCode ? ', including the text of the programs you run;' : ' (not the program text);'));
        }
        items.push('your screen size (rounded), language setting and time zone, and a random ID ' +
            'kept in this browser so visits can be linked. No name, e-mail, IP address or cookies.');
        return el('ul', {}, items.map(function (t) { return el('li', { text: t }); }));
    }
    function privacyLink(text) {
        return cfg.privacyUrl ? el('a', { href: cfg.privacyUrl, target: '_blank', rel: 'noopener', text: text }) : null;
    }

    var banner = null, panel = null, foot = null, panelOpener = null;

    function closeBanner() { if (banner) { banner.remove(); banner = null; } }

    // Where the bar goes: the page's slot (`bannerSlot`, e.g. a row the
    // playground reserves in its layout), else the top of mdBook's <main>
    // (it scrolls away with the chapter), else the top of <body>.
    function bannerHost() {
        var slot = cfg.bannerSlot ? document.querySelector(cfg.bannerSlot) : null;
        if (slot) return { parent: slot, before: slot.firstChild, boxed: false };
        var main = document.querySelector('main');
        if (main) return { parent: main, before: main.firstChild, boxed: true };
        return { parent: document.body, before: document.body.firstChild, boxed: false };
    }

    var slotTries = 0;
    function showBanner() {
        if (banner || blocked || !document.body) return;
        // The page's slot may not exist yet (the playground renders after
        // this script runs): wait for it for up to 5 s, then use the default.
        if (cfg.bannerSlot && !document.querySelector(cfg.bannerSlot) && slotTries++ < 50) {
            setTimeout(showBanner, 100);
            return;
        }
        ensureCss();
        var notice = cfg.consent === 'notice';
        var code = cfg.surface === 'playground' && cfg.collectCode ? ', the programs you run' : '';
        var summary = notice
            ? 'This site records how you use it, for research on learning Rust: pages you view, ' +
              'which parts of diagrams you point at and your pointer’s path' + code +
              ', with a random ID kept in this browser. No name, IP address or cookies.'
            : 'May this site record how you use it, for research on learning Rust? Pages you view, ' +
              'which parts of diagrams you point at and your pointer’s path' + code +
              ', with a random ID kept in this browser. Nothing is recorded unless you say yes.';
        var details = el('div', { id: 'rv-tlm-details', hidden: 'hidden' }, [
            whatList(),
            showStudy ? el('p', { 'class': 'rv-tlm-study', text: cfg.studyText }) : null
        ]);
        var more = el('button', { type: 'button', 'class': 'rv-tlm-plain', 'aria-expanded': 'false',
                                  'aria-controls': 'rv-tlm-details', text: 'What’s recorded' });
        more.addEventListener('click', function () {
            var open = details.hasAttribute('hidden');
            if (open) details.removeAttribute('hidden'); else details.setAttribute('hidden', 'hidden');
            more.setAttribute('aria-expanded', open ? 'true' : 'false');
        });
        var age = null;
        var actions = [more, privacyLink('Privacy')];
        if (!notice && cfg.study) {
            age = el('input', { type: 'checkbox', id: 'rv-tlm-age' });
            actions.push(el('span', {}, [age, ' ', el('label', { 'for': 'rv-tlm-age', text: 'I am 18+ and agree to take part' })]));
        }
        if (notice) {
            actions.push(el('button', { type: 'button', text: 'OK', onclick: function () { dismissNotice(); } }),
                         el('button', { type: 'button', text: 'Turn off', onclick: function () { withdraw(true); } }));
        } else {
            actions.push(el('button', { type: 'button', text: 'Yes, record', onclick: function () {
                             optIn(age ? age.checked : false); } }),
                         el('button', { type: 'button', text: 'No, thanks', onclick: function () { decline(); } }));
        }
        var host = bannerHost();
        banner = el('section', { 'class': 'rv-tlm rv-tlm-bar' + (host.boxed ? ' rv-tlm-boxed' : ''),
                                 role: 'region', 'aria-label': 'Usage logging' }, [
            el('div', { 'class': 'rv-tlm-line' }, [
                el('span', { 'class': 'rv-tlm-text', text: summary }),
                el('span', { 'class': 'rv-tlm-actions' }, actions)
            ]),
            details
        ]);
        host.parent.insertBefore(banner, host.before);
    }

    // A small "Your data" link at the end of the page's content, so the
    // panel stays reachable after the bar is gone (pages that link it
    // themselves set panelLink: 'none').
    function showFoot() {
        if (cfg.panelLink !== 'inline' || foot || !document.body) return;
        ensureCss();
        var main = document.querySelector('main') || document.body;
        foot = el('p', { 'class': 'rv-tlm-foot' }, [
            'This site records usage for research. ',
            el('button', { type: 'button', 'class': 'rv-tlm-link', 'aria-haspopup': 'dialog',
                           text: 'Your data', onclick: function () { openPanel(); } }),
            cfg.privacyUrl ? ' · ' : null,
            privacyLink('Privacy')
        ]);
        main.appendChild(foot);
    }

    function closePanel() {
        if (!panel) return;
        panel.remove();
        panel = null;
        if (panelOpener && panelOpener.focus) panelOpener.focus();
        panelOpener = null;
    }
    function openPanel() {
        if (!document.body) return;
        ensureCss();
        var opener = document.activeElement;
        closePanel();
        panelOpener = opener;
        emit('ui', { action: 'privacy_panel' });
        var id = anonId(false);
        var status;
        if (blocked) status = 'Your browser asks sites not to track you (Global Privacy Control or Do Not Track), ' +
            'so this site records nothing and stores no ID.';
        else if (active) status = 'Recording is on.';
        else status = 'Recording is off. Nothing is being recorded.';
        var kids = [
            el('button', { type: 'button', 'class': 'rv-tlm-close', 'aria-label': 'Close', text: '×',
                           onclick: closePanel }),
            el('h2', { id: 'rv-tlm-panel-title', text: 'Your data' }),
            el('p', { text: status })
        ];
        if (!blocked) {
            kids.push(el('p', { text: 'What is recorded while it is on:' }), whatList());
            if (showStudy) kids.push(el('p', { 'class': 'rv-tlm-study', text: cfg.studyText }));
            kids.push(el('p', {}, ['Your random ID: ', id ? el('code', { text: id }) : 'none (nothing stored)',
                                   id ? '. Quote it if you ask us about your data.' : '']));
        }
        var link = privacyLink('Privacy details');
        if (link) kids.push(el('p', {}, [link]));
        var row = [];
        if (!blocked && active) {
            row.push(el('button', { type: 'button', text: 'Turn off', onclick: function () { withdraw(false); closePanel(); } }));
            row.push(el('button', { type: 'button', text: 'Turn off and delete my data',
                                    onclick: function () { withdraw(true); closePanel(); } }));
        } else if (!blocked) {
            row.push(el('button', { type: 'button', text: 'Turn on', onclick: function () {
                closePanel();
                if (cfg.consent === 'opt_in') { removeItem('localStorage', 'choice'); choice = null; showBanner(); }
                else turnOnNotice();
            } }));
        }
        row.push(el('button', { type: 'button', text: 'Close', onclick: closePanel }));
        kids.push(el('div', { 'class': 'rv-tlm-row' }, row));
        panel = el('div', { 'class': 'rv-tlm rv-tlm-panel', role: 'dialog', 'aria-modal': 'false',
                            'aria-labelledby': 'rv-tlm-panel-title', tabindex: '-1' }, kids);
        panel.addEventListener('keydown', function (e) { if (e.key === 'Escape') closePanel(); });
        document.body.appendChild(panel);
        panel.focus();
    }

    // ------------------------------------------------------------------
    // Consent transitions
    // ------------------------------------------------------------------

    var started = false;
    beforeFlush.push(function () {
        // pointer_trace chunks go out with each batch
        for (var i = 0; i < figures.length; i++) closeChunk(figures[i]);
    });
    function start() {
        if (started || !active) return;
        started = true;
        scan();
        sendPageView();
    }

    function optIn(age18) {
        choice = { state: 'on', how: 'opt_in', version: cfg.consentVersion, age18: !!age18 };
        saveChoice(choice);
        closeBanner();
        active = true;
        var props = { consent_version: cfg.consentVersion, choice: 'opt_in' };
        if (cfg.study) props.age_18_plus = !!age18;
        emit('consent', props);
        start();
        showFoot();
        flush(false);
    }
    function decline() {
        choice = { state: 'off', how: 'declined', version: cfg.consentVersion };
        saveChoice(choice);
        closeBanner();
        showFoot();
    }
    function dismissNotice() {
        choice = { state: 'on', how: 'notice', version: cfg.consentVersion };
        saveChoice(choice);
        closeBanner();
        emit('consent', { consent_version: cfg.consentVersion, choice: 'notice_dismissed' });
        showFoot();
    }
    function turnOnNotice() {
        choice = { state: 'on', how: 'notice', version: cfg.consentVersion };
        saveChoice(choice);
        active = true;
        emit('consent', { consent_version: cfg.consentVersion, choice: 'notice_dismissed' });
        start();
        flush(false);
    }
    // Turn logging off: one last `withdraw` event (it carries the ids so the
    // data can be found and deleted), then forget the ids and stop.
    function withdraw(deleteRequested) {
        if (active) {
            closeAllFigures(now());
            emit('withdraw', { consent_version: cfg.consentVersion, delete_requested: !!deleteRequested });
            flush(true);
        }
        active = false;
        started = false;
        pageViewSent = false;
        queue.length = 0;
        retryOnce.length = 0;
        removeItem('localStorage', 'anon_id');
        removeItem('localStorage', 'session');
        removeItem('sessionStorage', 'cohort');
        cohort = null;
        choice = { state: 'off', how: 'withdrawn', version: cfg.consentVersion };
        saveChoice(choice);
        closeBanner();
        showFoot();
    }

    // ------------------------------------------------------------------
    // Wiring
    // ------------------------------------------------------------------

    function ui(action, value, sourceFigureId) {
        var props = { action: action };
        if (typeof value === 'string' && value) props.value = value.slice(0, 64);
        if (sourceFigureId && /^[0-9a-f]{16,64}$/.test(sourceFigureId)) props.source_figure_id = sourceFigureId;
        return emit('ui', props);
    }

    window.rvTelemetry = {
        version: CLIENT_VERSION,
        isActive: function () { return active; },
        emit: emit,
        ui: ui,
        tooltipHover: tooltipHover,
        program: program,
        programId: programId,
        programOmitted: function (id) { return omittedPrograms[id] === true; },
        figureId: figureId,
        scan: function () { if (active) scan(); },
        flush: function () { flush(false); },
        openPanel: openPanel,
        pageLoadId: function () { return active ? pageLoadId : null; },
        collectsCode: function () { return active && cfg.collectCode; }
    };
    if (window.__RV_TELEMETRY_TEST__) {
        window.rvTelemetry._test = {
            queue: queue, sha256: sha256, batches: batches, cfg: cfg, optIn: optIn, decline: decline,
            dismissNotice: dismissNotice, withdraw: withdraw, pageHide: pageHide, flush: flush
        };
    }

    if (blocked) return;   // GPC / DNT: no banner, no listeners, nothing sent

    function boot() {
        var notice = cfg.consent === 'notice';
        if (active) {
            if (notice && !choice) {
                // First page load under the notice: say so before anything else.
                saveChoice(choice = { state: 'on', how: 'notice_shown', version: cfg.consentVersion });
                emit('consent', { consent_version: cfg.consentVersion, choice: 'notice_shown' });
            }
            start();
        }
        if (notice && choice && choice.how === 'notice_shown') showBanner();
        else if (!notice && (!choice || (choice.how !== 'opt_in' && choice.how !== 'declined' &&
                                         choice.how !== 'withdrawn'))) showBanner();
        showFoot();
    }
    if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', boot);
    else boot();

    setInterval(function () { tick(); if (active) flush(false); }, FLUSH_MS);
    setInterval(tick, 1000);
    ['pointermove', 'pointerdown', 'keydown', 'wheel', 'touchstart'].forEach(function (t) {
        window.addEventListener(t, markInput, { passive: true, capture: true });
    });
    window.addEventListener('scroll', noteScroll, { passive: true });
    window.addEventListener('focus', function () { tick(); markInput(); });
    window.addEventListener('blur', tick);
    document.addEventListener('click', onClick, true);
    document.addEventListener('visibilitychange', function () {
        if (document.visibilityState === 'hidden') pageHide('hidden');
        else { lastTick = now(); markInput(); reopenVisibleFigures(); }
    });
    window.addEventListener('pagehide', function () { pageHide('unload'); });
})();
