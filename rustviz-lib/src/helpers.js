// Tooltip + cross-panel highlighting glue for RustViz visualizations.
//
// A "visualization" is a pair of SVGs that share a class name —
// `<… class="example-N code_panel">` for the code panel and
// `<… class="example-N tl_panel">` for the timeline. The two are
// either:
//   - <object> tags loading external .svg files (mdbook output), or
//   - inline <svg> elements (CLI --html output, single-file).
//
// `panelRoot()` papers over the difference: for an <object> we
// reach into its contentDocument; for an inline <svg> the element
// IS the root. Everything downstream uses element-scoped query
// methods (querySelector, getElementsByClassName on the root)
// instead of Document-scoped getElementById, so the same code runs
// in both modes without further branching.

// Same helper as the playground's copy. A panel is an <object> (mdbook), a
// bare inline <svg> (`rustviz html`), or a wrapper element holding an <svg>
// (playground).
function panelRoot(panel) {
    if (!panel) return null;
    if (panel.contentDocument) return panel.contentDocument.documentElement;
    if (panel.tagName && panel.tagName.toLowerCase() === 'svg') return panel;
    return panel.querySelector ? panel.querySelector('svg') : null;
}
function panelOwnerDocument(panel) {
    if (panel && panel.contentDocument) return panel.contentDocument;
    return document;
}

const SVG = {
    'text': {
        'label': 'label',
        'functionLogo': 'label'
    },
    'path': {
        'hollow': 'timeline_mut',
        'staticref': 'static_ref_line',
        'mutref': 'mut_ref_line'
    },
    'polyline': 'arrow',
    'circle': 'event',
    'line': 'timeline_mut',
    'use': 'function_event',
    'rect': 'structBox'
};

function helpers(classname) {
    // mdbook 0.4 wrapped the chapter in `<div id="page-wrapper">`;
    // 0.5 renamed it to `#mdbook-page-wrapper`. Try the 0.5 id
    // first, fall back to the legacy id, and finally `body` so a
    // future rename doesn't null-deref the next line.
    let page = document.querySelector('#mdbook-page-wrapper')
            || document.querySelector('#page-wrapper')
            || document.body;
    let tooltip = document.getElementById('svg_tooltip');
    if (!tooltip) {
        tooltip = document.createElement('p');
        tooltip.id = 'svg_tooltip';
        // max-width + overflow-wrap let the browser wrap long
        // tooltips for us — `breakText` used to do this in JS but
        // it tokenized on whitespace, which butchered the inline
        // `<span style="...monospace, monospace !important;">name`
        // wrappers (the cut would land inside the style attribute,
        // leaking attribute fragments into the visible text).
        tooltip.style.cssText = "position: absolute; padding: 0.5em; font-size: 0.75em; border-radius: 8px;" +
                                "font-family: 'Trebuchet MS', Helvetica, sans-serif;" +
                                "background: rgb(70, 70, 70, 0.6); color: white; z-index: 100; display: none;" +
                                "max-width: 360px; overflow-wrap: break-word;" +
                                // never intercept the mouse: if the tooltip sits under the cursor it would
                                // steal the hover, hiding itself and flickering
                                "pointer-events: none;" +
                                // pin the layout box to the page origin so the translate() below is
                                // measured from a known point, whatever margins the host page gives <p>
                                "top: 0; left: 0; margin: 0;";
        page.parentNode.insertBefore(tooltip, page);
    }

    displayFn(classname);
    displayTooltip(tooltip, classname);
}

function displayFn(classname) {
    let vis_num = document.getElementsByClassName(classname);
    let code_obj = vis_num[0];
    let tl_obj = vis_num[1];
    let c_svg = panelRoot(code_obj);
    let tl_svg = panelRoot(tl_obj);
    if (!c_svg || !tl_svg) return;
    let triggers = tl_svg.getElementsByClassName('fn-trigger');
    var functions = c_svg.getElementsByClassName('fn');

    for (let i = 0; i < triggers.length; i++) {
        // helpers() runs on every mouseenter of the container; don't stack listeners
        if (triggers[i].classList.contains('fn-listener')) continue;
        triggers[i].classList.add('fn-listener');
        triggers[i].addEventListener('mouseover', showFn);
        triggers[i].addEventListener('mouseout', hideFn);
    }

    function showFn(evt) {
        let evt_hash = evt.target.dataset.hash;

        for (let i = 0; i < functions.length; i++) {
            if (functions[i].getAttribute('hash') === evt_hash) {
                functions[i].dataset.hash = evt_hash;
            }
        }
    }

    function hideFn() {
        for (let i = 0; i < functions.length; i++) {
            functions[i].dataset.hash = 0;
        }
    }
}

function sizeToFit(object) {
    if (navigator.userAgent.indexOf("Chrome") !== -1) {
        object.addEventListener('load', function() {
            let svg_doc = object.contentDocument;
            let code_width = svg_doc.getElementById('code').getBBox().width;
            let new_width = Math.max(code_width + 30, 400);
            svg_doc.firstChild.setAttribute('width', new_width + 'px');
        }, {once: true});
    }
    else {
        if (object.contentDocument.readyState === "complete") {
            let svg_doc = object.contentDocument;
            let code_width = svg_doc.getElementById('code').getBBox().width;
            let new_width = Math.max(code_width + 30, 400);
            svg_doc.firstChild.setAttribute('width', new_width + 'px');
        }
    }
}


function displayTooltip(tooltip, classname) {
    let tl_obj = document.getElementsByClassName(classname)[1];
    let tl_svg = panelRoot(tl_obj);
    if (!tl_svg) return;
    let triggers = tl_svg.getElementsByClassName('tooltip-trigger');

    var time_start = null;

    for (let i = 0; i < triggers.length; i++) {
        // prevent adding duplicate listeners
        if (triggers[i].classList.contains('listener')) continue;
        triggers[i].classList.add('listener');

        triggers[i].addEventListener('mousemove', showTooltip);
        triggers[i].addEventListener('mouseleave', hideTooltip);
        triggers[i].addEventListener('mouseenter', insertUnderline);
    }

    function showTooltip(e) {
        if (!time_start) time_start = Date.now();

        let mouse = mousePos(e, tl_obj);
        tooltip.style.transform = "translate(" + mouse.x + "px, " + mouse.y + "px)";
        tooltip.style.display = "block";

        let text = e.currentTarget.getAttributeNS(null, "data-tooltip-text");
        tooltip.innerHTML = text;
        // Wrapping is handled by CSS (max-width + overflow-wrap on
        // the tooltip element itself); no JS string surgery here.
    }

    function hideTooltip(e) {
        tooltip.style.display = 'none';
        tooltip.innerHTML = '';

        let tgt = e.currentTarget;
        let e_label = (tgt.tagName === 'text') ? SVG['text'][tgt.classList[0]]
            : ((tgt.tagName === 'path') ? SVG['path'][tgt.classList[0]]
            : SVG[tgt.tagName]);

        time_start = null;

        removeUnderline(e, classname);
    }

    function insertUnderline(e) {
        let code_obj = document.getElementsByClassName(classname + ' code_panel')[0];
        let code_root = panelRoot(code_obj);
        if (!code_root) return;
        let begin = 0, end = 0;
        if (e.currentTarget.tagName === 'path') {
            let arr = e.currentTarget.getAttribute('d').split(' ');
            if (e.currentTarget.parentNode.id === 'ref_line') {
                begin = parseInt(arr[2]);
                end = parseInt(begin) + 2*parseInt(arr[5]) + parseInt(arr[7]) + 5;
            }
            else {
                let y1 = parseInt(arr[1].split(',')[1]);
                let y2 = parseInt(arr[3].split(',')[1]);
                begin = Math.min(y1, y2);
                end = Math.max(y1, y2);
            }
        }
        else if (e.currentTarget.tagName === 'line') {
            let y1 = e.currentTarget.getAttribute('y1');
            let y2 = e.currentTarget.getAttribute('y2');
            begin = Math.min(y1, y2);
            end = Math.max(y1, y2);
        }
        else {
            let pos;
            if (e.currentTarget.tagName === 'circle') {
                begin = end = parseInt(e.currentTarget.getAttribute('cy')) + 5;
            }
            else if (e.currentTarget.tagName === 'use') {
                begin = end = parseInt(e.currentTarget.getAttribute('y')) + 5;
            }
            else if (e.currentTarget.tagName === 'g') {
                // A group: a drop dot (circle + triangle), a merged call dot,
                // or an arrow (shaft + head). It has no y attribute of its
                // own: use its circle's centre, else the arrow's first point,
                // else (a hop arrow's path) the lines its box spans.
                let c = e.currentTarget.querySelector('circle');
                let pl = e.currentTarget.querySelector('polyline');
                if (c) {
                    begin = end = parseInt(c.getAttribute('cy')) + 5;
                } else if (pl) {
                    let arr = pl.getAttribute('points').split(' ');
                    begin = end = parseInt(arr[1]) + 5;
                } else {
                    let bb = e.currentTarget.getBBox();
                    begin = bb.y;
                    end = bb.y + bb.height + 5;
                }
            }
            else if (e.currentTarget.tagName === 'polyline') {
                let arr = e.currentTarget.getAttribute('points').split(' ');
                begin = end = parseInt(arr[1]) + 5;
            }
            else {
                begin = end = parseInt(e.currentTarget.getAttribute('y'));
            }
        }

        // Element-scoped lookup (querySelector) works on both SVG
        // root elements and Document, unlike getElementById.
        let code_g = code_root.querySelector('#code');
        let lines = code_g.children;
        let len = lines.length;
        for (let i=0; i<len; ++i) {
            let ly = parseInt(lines[i].getAttribute('y'));
            if (ly >= begin && ly <= end) {
                // Underline the whole line: one SVG line across the line's own
                // measured box. (It used to be a row of '_' sized by assuming
                // 8px per character, which fell short with the rendered font.)
                let box = lines[i].getBBox();
                let emph = panelOwnerDocument(code_obj).createElementNS('http://www.w3.org/2000/svg', 'line');
                emph.setAttribute('class', 'code emph');
                emph.setAttribute('x1', box.x);
                emph.setAttribute('x2', box.x + box.width);
                emph.setAttribute('y1', ly + 4);
                emph.setAttribute('y2', ly + 4);
                emph.setAttribute('stroke', 'currentColor');
                emph.setAttribute('stroke-width', '1');
                code_g.appendChild(emph);
            }
        }
    }
}


// Track mouse movement. With <object> embedding (mdbook-rustviz),
// evt.clientX/Y are local to the SVG document, so the panel's bounding rect
// maps them back to the page. With inline SVG (`rustviz html`, the
// playground), evt.clientX/Y are already viewport coordinates and only the
// scroll offset is needed (the tooltip is position:absolute).
function mousePos(evt, obj) {
    let x_pos, y_pos;
    if (obj && obj.contentDocument) {
        let r = obj.getBoundingClientRect();
        x_pos = evt.clientX + r.x + 15;
        y_pos = evt.clientY + r.y + window.scrollY + 45;
    } else {
        x_pos = evt.clientX + window.scrollX + 15;
        y_pos = evt.clientY + window.scrollY + 25;
    }
    return {
        x: Math.round(x_pos),
        y: Math.round(y_pos)
    };
}

function removeUnderline(e, classname) {
    let code_obj = document.getElementsByClassName(classname + ' code_panel')[0];
    let code_root = panelRoot(code_obj);
    if (!code_root) return;
    let arr = code_root.getElementsByClassName('emph');
    for (let i = arr.length-1; i >= 0; --i) {
        arr[i].remove();
    }
}


function toggleAll(turn_on) {
    let evt = new MouseEvent("click", {
      bubbles: true,
      cancelable: true,
      view: window
    });

    let arr = document.getElementsByClassName('toggle-button');
    for (const obj of arr) {
        if (turn_on && obj.classList.contains('fa-toggle-off')) {
            obj.dispatchEvent(evt);
        }
        else if (!turn_on && obj.classList.contains('fa-toggle-on')) {
            obj.dispatchEvent(evt);
        }
    }
}

function toggleStruct(turn_on) {
    var evt = new MouseEvent("click", {
      bubbles: true,
      cancelable: true,
      view: window
    });

    var arr = document.getElementsByClassName('non-struct');
    for (const obj of arr) {
        if (turn_on && obj.classList.contains('fa-toggle-off')) {
            obj.dispatchEvent(evt);
        }
        else if (!turn_on && obj.classList.contains('fa-toggle-on')) {
            obj.dispatchEvent(evt);
        }
    }
}
