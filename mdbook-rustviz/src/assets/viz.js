// RustViz next to Aquascope: tabs, theming of the RustViz SVGs, and the
// prototype combined views. Classic script; may be injected once per
// chapter, so guard against double initialisation.
(function () {
  if (window.__rvViz) return;
  window.__rvViz = true;
  var SVGNS = 'http://www.w3.org/2000/svg';

  // ---- tabs -----------------------------------------------------------
  document.addEventListener('click', function (e) {
    var b = e.target.closest && e.target.closest('.rv-viz-tab');
    if (!b) return;
    var box = b.closest('.rv-viz');
    var k = b.getAttribute('data-pane');
    box.querySelectorAll(':scope > .rv-viz-bar > .rv-viz-tab').forEach(function (t) {
      t.setAttribute('aria-selected', t === b ? 'true' : 'false');
    });
    box.querySelectorAll(':scope > .rv-viz-panes > .rv-viz-pane').forEach(function (p) {
      p.setAttribute('aria-hidden', p.getAttribute('data-pane') === k ? 'false' : 'true');
    });
  });

  // ---- theme: RustViz SVGs live in <object>s and don't see page CSS, so
  // copy the theme's code-block colours into each SVG document. ---------
  function rgb(c) {
    var m = /rgba?\(([^)]+)\)/.exec(c || '');
    return m ? m[1].split(',').map(parseFloat) : null;
  }
  // One persistent probe element (re-inserting one on every theme change
  // would feed Font Awesome's MutationObserver, which re-sets <html class>).
  var probe = null;
  function palette() {
    if (!probe) {
      var pre = document.createElement('pre');
      pre.setAttribute('aria-hidden', 'true');
      pre.style.cssText = 'position:absolute;left:-9999px;top:0;visibility:hidden;';
      probe = document.createElement('code');
      probe.className = 'hljs';
      pre.appendChild(probe);
      (document.querySelector('#mdbook-content') || document.body).appendChild(pre);
    }
    var cs = getComputedStyle(probe);
    var bg = cs.backgroundColor, fg = cs.color;
    var p = rgb(bg);
    if (!p || p[3] === 0) { bg = getComputedStyle(document.body).backgroundColor; p = rgb(bg); }
    var dark = p ? (0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2]) / 255 < 0.4 : false;
    var font = document.querySelector('link[href*="fonts/fonts"]');
    return { bg: bg, fg: fg, dark: dark, fonts: font ? font.href : null };
  }
  function svgCss(pal) {
    var s = (pal.fonts ? '@import url("' + pal.fonts + '");\n' : '') +
      ':root { --bg-color: ' + pal.bg + '; }\n';
    if (pal.dark) {
      s += 'text.code { fill: ' + pal.fg + '; fill-opacity: 0.9; }\n' +
        'text.code .comment { fill: #8b8d9c; }\n' +
        'text.code .string { fill: #8fb6ef; }\n' +
        '#caption { fill: ' + pal.fg + '; fill-opacity: 0.75; }\n' +
        '[data-hash="0"] { fill: ' + pal.fg + '; }\n' +
        '[data-hash="11"] { fill: ' + pal.fg + '; stroke: ' + pal.fg + '; }\n' +
        '[data-hash="3"] { fill: #a87bff; stroke: #a87bff; }\n' +
        '[data-hash="5"] { fill: #43c443; stroke: #43c443; }\n' +
        '[data-hash="6"] { fill: #26b5b5; stroke: #26b5b5; }\n' +
        '[data-hash="12"] { fill: #8f88ff; stroke: #8f88ff; }\n' +
        '[fill="gray"] { fill: #9a9cab; } [stroke="gray"] { stroke: #9a9cab; }\n';
    }
    return s;
  }
  function styleObject(obj, pal) {
    var d = obj.contentDocument;
    if (!d || !d.documentElement || d.documentElement.nodeName.toLowerCase() !== 'svg') return false;
    var st = d.getElementById('rv-theme');
    if (!st) {
      st = d.createElementNS(SVGNS, 'style');
      st.id = 'rv-theme';
      d.documentElement.appendChild(st);
    }
    st.textContent = svgCss(pal);
    return true;
  }
  var PAL = null;
  function restyleAll() {
    PAL = palette();
    document.documentElement.style.setProperty('--rv-code-bg', PAL.bg);
    document.querySelectorAll('.rv-fig object').forEach(function (o) { styleObject(o, PAL); });
  }
  function hookObjects() {
    document.querySelectorAll('.rv-fig object').forEach(function (o) {
      o.addEventListener('load', function () { styleObject(o, PAL || palette()); fitFig(o.closest('.rv-fig')); combosFor(o); });
    });
  }
  // mdBook switches themes by changing <html class="...">.
  // Only react to a real change: classList.add() of a class that is
  // already present still produces a mutation record.
  var lastClass = null;
  new MutationObserver(function () {
    var c = document.documentElement.className;
    if (c === lastClass) return;
    lastClass = c;
    restyleAll();
  }).observe(document.documentElement, { attributes: true, attributeFilter: ['class'] });


  // ---- fit: scale the two panels down (never up) so code + timeline fit
  // the text column instead of clipping the code; below 60% scroll instead.
  function svgRoot(o) {
    var d = o.contentDocument;
    return d && d.documentElement && d.documentElement.nodeName.toLowerCase() === 'svg' ? d.documentElement : null;
  }
  function natural(o) {
    var r = svgRoot(o);
    if (!r) return null;
    if (!o.__rvNat) {
      var w = parseFloat(r.getAttribute('width')) || 400, h = parseFloat(r.getAttribute('height')) || 300;
      try { var bb = r.getBBox(); w = Math.max(w, Math.ceil(bb.x + bb.width + 12)); } catch (e) {}
      o.__rvNat = { w: w, h: h };
    }
    return o.__rvNat;
  }
  function fitFig(fig) {
    var objs = fig.querySelectorAll('object');
    if (objs.length !== 2) return;
    var a = natural(objs[0]), b = natural(objs[1]);
    if (!a || !b) return;
    var g = objs[0].__rvGutter || 0;
    var total = a.w + g + b.w;
    var avail = fig.clientWidth || total;
    var k = Math.max(0.6, Math.min(1, avail / total));
    [[objs[0], a, g], [objs[1], b, 0]].forEach(function (t) {
      var o = t[0], n = t[1], gut = t[2], r = svgRoot(o);
      r.setAttribute('viewBox', (-gut) + ' 0 ' + (n.w + gut) + ' ' + n.h);
      r.setAttribute('width', ((n.w + gut) * k) + 'px');
      r.setAttribute('height', (n.h * k) + 'px');
      o.style.width = ((n.w + gut) * k) + 'px';
      o.style.height = (n.h * k) + 'px';
    });
    fig.style.height = (Math.max(a.h, b.h) * k) + 'px';
  }
  function fitAll() { document.querySelectorAll('.rv-fig').forEach(fitFig); }
  var fitTimer = null;
  window.addEventListener('resize', function () { clearTimeout(fitTimer); fitTimer = setTimeout(fitAll, 100); });

  // ---- combined views (prototype) ---------------------------------------
  // Aquascope's step list, replicated from its `filterSteps`: for each
  // `[]` mark (a char offset in the code), the latest step whose frame
  // contains the mark and started before it. L-numbers follow execution.
  function aqSteps(embed) {
    var code = JSON.parse(embed.dataset.code);
    var ann = JSON.parse(embed.dataset.annotations);
    var resp = JSON.parse(embed.dataset.responses);
    var trace = resp.interpreter && resp.interpreter.Ok;
    if (!trace) return [];
    var starts = [0];
    for (var i = 0; i < code.length; i++) if (code[i] === '\n') starts.push(i + 1);
    var pos = function (p) { return starts[p.line] + p.column; };
    var lineOf = function (idx) { var l = 0; while (l + 1 < starts.length && starts[l + 1] <= idx) l++; return l; };
    var steps = trace.steps;
    var marks = ann.interp.state_locations.map(function (idx) {
      for (var j = steps.length - 1; j >= 0; j--) {
        var fr = steps[j].stack.frames[steps[j].stack.frames.length - 1];
        if (pos(fr.body_span.start) <= idx && idx <= pos(fr.body_span.end) && idx > pos(fr.location.start)) {
          var l = lineOf(idx);
          var before = code.slice(starts[l], idx).trim() === '';
          return { stepIdx: j, idx: idx, line: l + 1, before: before };
        }
      }
      return null;
    }).filter(Boolean);
    marks.sort(function (a, b) { return a.stepIdx - b.stepIdx; });
    marks.forEach(function (m, i) { m.label = 'L' + (i + 1); m.n = i; });
    return marks;
  }
  // RustViz rows: read them from the code panel (the plugin inserts
  // unnumbered rows, e.g. for events between two lines; they belong to the
  // line above). Row centre = text baseline - 5, rows are 30px tall.
  function rowMap(codeDoc) {
    var rows = [], cur = 0;
    codeDoc.querySelectorAll('text.code').forEach(function (t) {
      var num = t.querySelector('tspan');
      var n = num ? parseInt(num.textContent.trim(), 10) : NaN;
      if (!isNaN(n)) cur = n;
      rows.push({ y: parseFloat(t.getAttribute('y')) - 5, line: cur, numbered: !isNaN(n) });
    });
    return {
      lineY: function (n) { for (var i = 0; i < rows.length; i++) if (rows[i].numbered && rows[i].line === n) return rows[i].y; return 85 + 30 * (n - 1); },
      lastY: function (n) { var y = null; rows.forEach(function (r) { if (r.line === n) y = r.y; }); return y === null ? 85 + 30 * (n - 1) : y; },
      lineOf: function (y) { var best = null; rows.forEach(function (r) { if (!best || Math.abs(r.y - y) < Math.abs(best.y - y)) best = r; }); return best && Math.abs(best.y - y) <= 15 ? best.line : null; }
    };
  }
  var RM = null;

  function combosFor(obj) {
    var box = obj.closest('.rv-combo');
    if (!box) return;
    var aqEmbed = box.querySelector('.rv-combo-aq .aquascope-embed, .rv-combo-aq [data-responses]');
    var objs = box.querySelectorAll('.rv-fig object');
    var ready = Array.prototype.every.call(objs, function (o) { return o.contentDocument && o.contentDocument.documentElement && o.contentDocument.documentElement.nodeName.toLowerCase() === 'svg'; });
    var steps = box.querySelectorAll('.rv-combo-aq .interpreter .step');
    if (!ready || !aqEmbed || steps.length === 0) { setTimeout(function () { combosFor(obj); }, 200); return; }
    if (box.__rvDone) return;
    box.__rvDone = true;
    var marks = aqSteps(aqEmbed);
    RM = rowMap(box.querySelector('object.code_panel').contentDocument);
    var rm = RM;
    var kind = box.getAttribute('data-combo');
    var cap = document.createElement('div');
    cap.className = 'rv-combo-cap';
    var bandEls = [];
    // a highlight band per SVG, moved to the hovered/selected row
    objs.forEach(function (o) {
      var d = o.contentDocument, root = d.documentElement;
      var band = d.createElementNS(SVGNS, 'rect');
      band.setAttribute('x', 0); band.setAttribute('width', '100%'); band.setAttribute('height', 30);
      band.setAttribute('fill', PAL && PAL.dark ? 'rgba(255,255,255,0.07)' : 'rgba(0,0,0,0.06)');
      band.setAttribute('visibility', 'hidden');
      band.setAttribute('pointer-events', 'none');
      root.insertBefore(band, root.firstChild.nextSibling);
      bandEls.push(band);
      if (kind === 'rows') drawRules(o, marks, rm);
    });
    function showBand(line) {
      bandEls.forEach(function (b) {
        if (line == null) { b.setAttribute('visibility', 'hidden'); return; }
        var y0 = rm.lineY(line) - 15, y1 = rm.lastY(line) + 15;
        b.setAttribute('y', y0); b.setAttribute('height', y1 - y0); b.setAttribute('visibility', 'visible');
      });
    }
    function select(m, line) {
      steps.forEach(function (s, i) { s.classList.toggle('rv-on', !!m && i === m.n); });
      box.classList.toggle('rv-focus', !!m);
      if (kind === 'hover') {
        cap.textContent = m ? (m.label + ': memory ' + (m.before ? 'before' : 'after') + ' line ' + m.line + (line && line !== m.line ? ' (latest snapshot at or before line ' + line + ')' : ''))
          : (line ? 'No Aquascope snapshot at or before line ' + line : 'Hover a line to see Aquascope’s stack & heap at that point');
      }
    }
    // the latest mark at or before the end of `line`, in text order
    function markForLine(line) {
      var best = null;
      marks.forEach(function (m) { if (m.line < line || (m.line === line)) { if (!best || m.idx > best.idx) best = m; } });
      return best;
    }
    if (kind === 'hover') {
      box.querySelector('.rv-combo-aq').prepend(cap);
      objs.forEach(function (o) {
        var d = o.contentDocument;
        d.addEventListener('mousemove', function (ev) {
          var p = d.documentElement.createSVGPoint(); p.x = ev.clientX; p.y = ev.clientY;
          var q = p.matrixTransform(d.documentElement.getScreenCTM().inverse());
          var line = rm.lineOf(q.y);
          if (!line) { showBand(null); return; }
          showBand(line);
          select(markForLine(line), line);
        });
      });
      select(marks[0], null);
      cap.textContent = 'Hover a line of the timeline to see Aquascope’s stack & heap there (showing ' + (marks[0] ? marks[0].label : '-') + ')';
    } else {
      // rows: hover a rule/badge or an Aquascope step to link the two
      steps.forEach(function (s, i) {
        s.addEventListener('mouseenter', function () { var m = marks[i]; select(m); showBand(m ? m.line : null); });
        s.addEventListener('mouseleave', function () { select(null); showBand(null); });
      });
      objs.forEach(function (o) {
        o.contentDocument.querySelectorAll('.rv-step').forEach(function (g) {
          var m = marks[+g.getAttribute('data-n')];
          g.addEventListener('mouseenter', function () { select(m); showBand(m.line); });
          g.addEventListener('mouseleave', function () { select(null); showBand(null); });
        });
      });
    }
  }
  // Aquascope step markers as rows across both RustViz panels.
  var GUTTER = 36; // room for the L-badges left of the code panel
  function drawRules(o, marks, rm) {
    var d = o.contentDocument, root = d.documentElement;
    var isTl = o.classList.contains('tl_panel');
    var nat = natural(o) || { w: 400 };
    var w = nat.w;
    var x0 = 0;
    if (!isTl) {
      // widen the code panel to the left to make a gutter for the badges
      o.__rvGutter = GUTTER;
      x0 = -GUTTER;
      fitFig(o.closest('.rv-fig'));
    }
    var stroke = PAL && PAL.dark ? '#9a9cab' : '#777';
    marks.forEach(function (m) {
      var y = m.before ? rm.lineY(m.line) - 15 : rm.lastY(m.line) + 15;
      var g = d.createElementNS(SVGNS, 'g');
      g.setAttribute('class', 'rv-step');
      g.setAttribute('data-n', m.n);
      var ln = d.createElementNS(SVGNS, 'line');
      ln.setAttribute('x1', isTl ? 0 : x0 + 31); ln.setAttribute('x2', w); ln.setAttribute('y1', y); ln.setAttribute('y2', y);
      ln.setAttribute('stroke', stroke); ln.setAttribute('stroke-width', 1); ln.setAttribute('stroke-dasharray', '4 3');
      g.appendChild(ln);
      if (!isTl) {
        // badge in the gutter, styled like Aquascope's step marker
        var bx = x0 + 3;
        var r = d.createElementNS(SVGNS, 'rect');
        r.setAttribute('x', bx); r.setAttribute('y', y - 9); r.setAttribute('width', 26); r.setAttribute('height', 18); r.setAttribute('rx', 4);
        r.setAttribute('fill', PAL ? PAL.bg : '#fff'); r.setAttribute('stroke', PAL && PAL.dark ? PAL.fg : '#333');
        var t = d.createElementNS(SVGNS, 'text');
        t.setAttribute('x', bx + 13); t.setAttribute('y', y + 4.5); t.setAttribute('text-anchor', 'middle');
        t.setAttribute('font-size', '12'); t.setAttribute('font-family', '"Source Code Pro", monospace');
        t.setAttribute('fill', PAL && PAL.dark ? PAL.fg : '#333');
        t.textContent = m.label;
        g.appendChild(r); g.appendChild(t);
      }
      root.appendChild(g);
    });
  }

  new MutationObserver(function () { clearTimeout(fitTimer); fitTimer = setTimeout(fitAll, 250); })
    .observe(document.body, { attributes: true, attributeFilter: ['class'] });
  function init() {
    lastClass = document.documentElement.className;
    PAL = palette();
    document.documentElement.style.setProperty('--rv-code-bg', PAL.bg);
    hookObjects();
    restyleAll();
    fitAll();
    document.querySelectorAll('.rv-combo .rv-fig object').forEach(function (o) { combosFor(o); });
  }
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', init); else init();
})();
