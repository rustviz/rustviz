// Hover/tooltip layer for the RustViz playground.
//
// Originally written for mdbook, where SVGs are embedded via <object>. The
// playground (rv-serve) inlines SVG into <div>s instead, so a few accesses
// have to branch:
//   * panelRoot()/panelOwnerDocument() — handle both contentDocument (object)
//     and querySelector('svg')/document (inline).
//   * mousePos() — evt.clientX is local-to-SVG with <object>, viewport with
//     inline; only add the panel offset in the first case.
//   * The tooltip <p>'s inline cssText pins top/left/margin/max-width because
//     the playground's index.css applies a global p {…} rule that would
//     otherwise shift the layout box and put it ~50px off the cursor.
// See memory: project_rv_serve_helpers_quirks.md.

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
    // Arrows are <g> wrappers around <polyline> + <polygon> so the
    // shaft and head share a single hover region. (Older standalone
    // <polyline> arrows kept here for any pre-RV2 SVGs that still
    // use them.)
    'g': 'arrow',
    'polyline': 'arrow',
    'circle': 'event',
    'line': 'timeline_mut',
    'use': 'function_event',
    'rect': 'structBox'
};

// The playground embeds SVGs inline (<div class="ex2 code_panel"><svg>...</svg></div>)
// while mdbook embeds them via <object data="vis_*.svg">. These helpers paper
// over the difference so the same file works in both contexts.
function panelRoot(panel) {
    if (!panel) return null;
    if (panel.contentDocument) return panel.contentDocument.documentElement;
    return panel.querySelector ? panel.querySelector('svg') : null;
}
function panelOwnerDocument(panel) {
    if (panel && panel.contentDocument) return panel.contentDocument;
    return document;
}

/* --------------------- SIMPLE DRIVER --------------------- */
function helpers(classname) {
    // create tooltip element; place it before #page-wrapper if present, else at body end
    let page = document.querySelector('#page-wrapper');
    let tooltip = document.getElementById('svg_tooltip');
    if (!tooltip) {
        tooltip = document.createElement('p');
        tooltip.id = 'svg_tooltip';
        // Long tooltips wrap in CSS (max-width + overflow-wrap); a JS word-splitter
        // used to do this and cut inside the <span style=...> name wrappers (#308).
        // top/left/margin/max-width pinned explicitly because the playground's
        // index.css applies `p { max-width: 800px; margin-left: 50px; }` and
        // the UA stylesheet adds vertical margins — without these overrides
        // `transform: translate(...)` would land far from the cursor.
        tooltip.style.cssText = "position: absolute; top: 0; left: 0; margin: 0; max-width: 360px; overflow-wrap: break-word;" +
                                "padding: 0.5em; font-size: 0.75em; border-radius: 8px; pointer-events: none;" +
                                "font-family: 'Trebuchet MS', Helvetica, sans-serif;" +
                                "background: rgb(70, 70, 70, 0.6); color: white; z-index: 100; display: none;";
        if (page && page.parentNode) page.parentNode.insertBefore(tooltip, page);
        else document.body.appendChild(tooltip);
    }

    displayFn(classname);
    displayTooltip(tooltip, classname);
}

/* --------------------- FUNCTION HIGHLIGHT --------------------- */

// change function name color on hover
function displayFn(classname) {
    // get svg elements
    let vis_num = document.getElementsByClassName(classname);
    let code_obj = vis_num[0];
    let tl_obj = vis_num[1];
    let c_svg = panelRoot(code_obj);
    let tl_svg = panelRoot(tl_obj);
    if (!c_svg || !tl_svg) return;
    // get elements that will trigger function
    let triggers = tl_svg.getElementsByClassName('fn-trigger');
    var functions = c_svg.getElementsByClassName('fn');

    for (let i = 0; i < triggers.length; i++) {
        triggers[i].addEventListener('mouseover', showFn);
        triggers[i].addEventListener('mouseout', hideFn);
    }

    function showFn(evt) {
        // get target attributes
        let evt_hash = evt.target.dataset.hash;

        for (let i = 0; i < functions.length; i++) {
            // if hashes match, temporarily change color
            if (functions[i].getAttribute('hash') === evt_hash) {
                functions[i].dataset.hash = evt_hash;
            }
        }
    }

    function hideFn() {
        // reset to hash 0, styling to black on mouseout
        for (let i = 0; i < functions.length; i++) {
            functions[i].dataset.hash = 0;
        }
    }
}

/* --------------------- SVG CODE-RELATED FUNCTIONS --------------------- */

// resize code block to fit comments
function sizeToFit(object) {
    // Case for Chrome loading
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

/* --------------------- TOOLTIP-RELATED FUNCTIONS --------------------- */

// change tooltip text on hover
function displayTooltip(tooltip, classname) {
    // get svg elements
    let panels = document.getElementsByClassName(classname);
    let code_obj = panels[0];
    let tl_obj = panels[1];
    let tl_svg = panelRoot(tl_obj);
    if (!tl_svg) return;
    // get elements that will trigger function
    let triggers = tl_svg.getElementsByClassName('tooltip-trigger');

    // track time
    var time_start = null;

    for (let i = 0; i < triggers.length; i++) {
        // prevent adding duplicate listeners
        if (triggers[i].classList.contains('listener')) break;
        else triggers[i].classList.add('listener');

        triggers[i].addEventListener('mousemove', showTooltip);
        triggers[i].addEventListener('mouseleave', hideTooltip);
        triggers[i].addEventListener('mouseenter', insertUnderline);
    }

    function showTooltip(e) {
        // only set time once, prevent from changing every time mouse moves
        if (!time_start) time_start = Date.now();

        // Fill in the text and show the box first: its size decides where it
        // can go without leaving the viewport.
        let text = e.currentTarget.getAttributeNS(null, "data-tooltip-text");
        tooltip.innerHTML = text;
        tooltip.style.display = "block";

        let pos = placeTooltip(tooltip, mousePos(e, tl_obj));
        tooltip.style.transform = "translate(" + pos.x + "px, " + pos.y + "px)";
    }

    function hideTooltip(e) {
        tooltip.style.display = 'none';
        tooltip.innerHTML = '';

        let tgt = e.currentTarget;
        let e_label = (tgt.tagName === 'text') ? SVG['text'][tgt.classList[0]]
            : ((tgt.tagName === 'path') ? SVG['path'][tgt.classList[0]]
            : SVG[tgt.tagName]);

        // only track hovering after mouse leaves element (gtag may be a no-op shim)
        if (typeof gtag === 'function') {
            gtag('event', 'tooltip_hover', { 'event_label': e_label });
            gtag('event', e_label, { 'hover_time': (Date.now() - time_start) });
        }
        time_start = null; // reset

        removeUnderline(e, classname, code_obj);
    }

    /* ---- SHOW RELEVANT LINES ---- */
    function insertUnderline(e) {
        // A branch column's diagonal from the split or to the join
        // passes other arms' lines; it underlines nothing (#369).
        if (e.currentTarget.getAttribute('data-underline') === 'none') return;
        // A branch column's hover polygon names the y-range of its
        // arm's own lines.
        let underline_y = e.currentTarget.getAttribute('data-underline-y');
        let codeRoot = panelRoot(code_obj);
        let ownerDoc = panelOwnerDocument(code_obj);
        if (!codeRoot) return;
        let codeGroup = codeRoot.getElementById ? codeRoot.getElementById('code') : codeRoot.querySelector('#code');
        if (!codeGroup) return;
        let begin = 0, end = 0;
        let by_row = false;
        if (underline_y) {
            let ys = underline_y.split(' ').map(parseFloat);
            // The range names rows (a line's text y - 5), not text ys: a
            // line belongs to the arm when its row lies inside it, both
            // ends inclusive. The match below compares rows.
            by_row = true;
            begin = Math.min(ys[0], ys[1]);
            end = Math.max(ys[0], ys[1]);
        }
        else if (e.currentTarget.tagName === 'path') {
            let arr = e.currentTarget.getAttribute('d').split(' ');
            if (e.currentTarget.parentNode.id === 'ref_line') {
                begin = parseInt(arr[2]);
                end = parseInt(begin) + 2*parseInt(arr[5]) + parseInt(arr[7]) + 5; // + 5 to include last line
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
            else { // e.currentTarget.tagName === 'text'
                begin = end = parseInt(e.currentTarget.getAttribute('y'));
            }
        }

        // add underlining to every relevant line
        let lines = codeGroup.children;
        let len = lines.length; // prevent len from changing
        for (let i=0; i<len; ++i) {
            let ly = parseInt(lines[i].getAttribute('y'));
            let in_range = by_row ? (ly - 5 >= begin - 2 && ly - 5 <= end + 2)
                                  : (ly >= begin && ly <= end);
            if (in_range) { // only underline relevant code
                // Underline the whole line: one SVG line across the line's own
                // measured box. (It used to be a row of '_' sized by assuming
                // 8px per character, which fell short with the rendered font.)
                let box = lines[i].getBBox();
                let emph = ownerDoc.createElementNS('http://www.w3.org/2000/svg', 'line');
                emph.setAttribute('class', 'code emph');
                emph.setAttribute('x1', box.x);
                emph.setAttribute('x2', box.x + box.width);
                emph.setAttribute('y1', ly + 4);
                emph.setAttribute('y2', ly + 4);
                emph.setAttribute('stroke', 'currentColor');
                emph.setAttribute('stroke-width', '2');
                codeGroup.appendChild(emph);
            }
        }
    }
}

// Where the pointer is, in viewport coordinates. With <object> embedding
// (mdbook-rustviz), evt.clientX/Y are local to the SVG document, so the
// panel's bounding rect maps them into the viewport. With inline SVG
// (`rustviz html`, the playground), they already are viewport coordinates.
function mousePos(evt, obj) {
    if (obj && obj.contentDocument) {
        let r = obj.getBoundingClientRect();
        return { x: evt.clientX + r.x, y: evt.clientY + r.y };
    }
    return { x: evt.clientX, y: evt.clientY };
}

// Gaps between the pointer and the tooltip box, like a native tooltip: by
// default 12px right of and 16px below the pointer; flipped left its right
// edge is 12px left of the pointer, flipped up its bottom edge 8px above it.
const TOOLTIP_DX = 12, TOOLTIP_DY = 16, TOOLTIP_UP_GAP = 8;

function placeTooltip(tooltip, mouse) {
    const margin = 8;
    const vw = document.documentElement.clientWidth || window.innerWidth;
    const vh = window.innerHeight || document.documentElement.clientHeight;
    const w = tooltip.offsetWidth;
    const h = tooltip.offsetHeight;
    let x = mouse.x + TOOLTIP_DX;
    if (x + w > vw - margin) x = mouse.x - w;
    x = Math.max(margin, Math.min(x, vw - margin - w));
    let y = mouse.y + TOOLTIP_DY;
    if (y + h > vh - margin) y = mouse.y - TOOLTIP_UP_GAP - h;
    y = Math.max(margin, Math.min(y, vh - margin - h));
    return {
        x: Math.round(x + window.scrollX),
        y: Math.round(y + window.scrollY)
    };
}

function removeUnderline(e, classname, code_obj) {
    let panel = code_obj || document.getElementsByClassName(classname + ' code_panel')[0];
    let root = panelRoot(panel);
    if (!root) return;
    let arr = root.getElementsByClassName('emph');
    for (let i = arr.length-1; i >= 0; --i) {
        arr[i].remove();
    }
}

/* --------------- TOGGLE ALL SVGS --------------- */
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

/* --------------- TOGGLE ALL STRUCTS --------------- */
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
