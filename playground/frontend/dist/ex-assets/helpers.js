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
        // top/left/margin/max-width pinned explicitly because the playground's
        // index.css applies `p { max-width: 800px; margin-left: 50px; }` and
        // the UA stylesheet adds vertical margins — without these overrides
        // `transform: translate(...)` would land far from the cursor.
        tooltip.style.cssText = "position: absolute; top: 0; left: 0; margin: 0; max-width: none;" +
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

        let mouse = mousePos(e, tl_obj);
        tooltip.style.transform = "translate(" + mouse.x + "px, " + mouse.y + "px)";
        tooltip.style.display = "block";

        let text = e.currentTarget.getAttributeNS(null, "data-tooltip-text");
        tooltip.innerHTML = text;

        // if out of bounds, break text into two lines
        if (tooltip.getBoundingClientRect().right >= document.body.clientWidth) breakText(text, tooltip);
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
        let codeRoot = panelRoot(code_obj);
        let ownerDoc = panelOwnerDocument(code_obj);
        if (!codeRoot) return;
        let codeGroup = codeRoot.getElementById ? codeRoot.getElementById('code') : codeRoot.querySelector('#code');
        if (!codeGroup) return;
        let begin = 0, end = 0;
        if (e.currentTarget.tagName === 'path') {
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
            else if (e.currentTarget.tagName === 'polyline') {
                let arr = e.currentTarget.getAttribute('points').split(' ');
                begin = end = parseInt(arr[1]) + 5;
            }
            else if (e.currentTarget.tagName === 'g') {
                // RV2 arrow wrapper: <g><polyline/><polygon/></g>.
                // Read the y-coord from the inner polyline, same
                // shape as the standalone-polyline branch above.
                let polyline = e.currentTarget.querySelector('polyline');
                if (polyline) {
                    let arr = polyline.getAttribute('points').split(' ');
                    begin = end = parseInt(arr[1]) + 5;
                }
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
            if (ly >= begin && ly <= end) { // only underline relevant code
                let emph = ownerDoc.createElementNS('http://www.w3.org/2000/svg', 'text');
                emph.setAttribute('class', 'code emph');
                emph.setAttribute('x', '25');
                emph.setAttribute('y', ly + 3); // +3 to hang just under text
                emph.innerHTML = new Array(
                    Math.floor(lines[i].getBBox().width/8) // size of '_' = 8
                ).join('_'); // string with all underscores
                codeGroup.appendChild(emph);
            }
        }
    }
}

// track mouse movement. With <object> embedding, evt.clientX/Y are local to
// the SVG document, so we add the panel's bounding rect to map back to the
// page. With inline SVG (the playground), evt.clientX/Y are already in
// viewport coordinates — we only need to add scroll to anchor against
// document.body (since the tooltip is position:absolute with no positioned
// ancestor).
function mousePos(evt, obj) {
    if (obj && obj.contentDocument) {
        let x_pos = evt.clientX + obj.getBoundingClientRect().x + 15;
        let y_pos = evt.clientY + obj.getBoundingClientRect().y + window.scrollY + 45;
        return { x: Math.round(x_pos), y: Math.round(y_pos) };
    }
    let x_pos = evt.clientX + window.scrollX + 15;
    let y_pos = evt.clientY + window.scrollY + 25;
    return { x: Math.round(x_pos), y: Math.round(y_pos) };
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

// adjust text box
function breakText(text, tooltip) {
    // combine span into one element
    let split_text = text.split(' ');
    let words = [];
    let last = 0, span = false;
    for(const elt of split_text) {
        if (elt.startsWith('<')) {
            span = true;
            words.push(elt);
            last = words.length-1;
        }
        else if (elt.startsWith('!important')) {
            span = false;
            words[last] += elt;
        }
        else {
            if (span) {
                words[last] = words[last] + ' ' + elt;
            }
            else {
                words.push(elt);
            }
        }
    }

    // adjust size and split text based on page boundary
    tooltip.innerHTML = '';
    let left = tooltip.getBoundingClientRect().left;
    for (const word of words) {
        tooltip.innerHTML += (word + ' ');
        if (left + tooltip.clientWidth > document.body.clientWidth - 20) {
            // reset tooltip text and break into new lines
            let idx = tooltip.innerHTML.lastIndexOf(' ', tooltip.innerHTML.length-2);
            let temp = tooltip.innerHTML.substr(0, idx);
            let other = tooltip.innerHTML.substr(idx + 1);

            tooltip.innerHTML = '';
            tooltip.innerHTML += temp;
            tooltip.innerHTML += ('<br />' + other);
        }
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
