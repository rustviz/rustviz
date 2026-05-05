// Tooltip + cross-panel highlighting glue for RustViz visualizations.
//
// A "visualization" is a pair of SVGs that share a class name —
// `<… class="example-N code_panel">` for the code panel and
// `<… class="example-N tl_panel">` for the timeline. The two are
// either:
//   - <object> tags loading external .svg files (mdbook output), or
//   - inline <svg> elements (CLI --html output, single-file).
//
// `getSvgRoot()` papers over the difference: for an <object> we
// reach into its contentDocument; for an inline <svg> the element
// IS the root. Everything downstream uses element-scoped query
// methods (querySelector, getElementsByClassName on the root)
// instead of Document-scoped getElementById, so the same code runs
// in both modes without further branching.

function getSvgRoot(elt) {
    return elt.contentDocument ? elt.contentDocument.documentElement : elt;
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
                                "max-width: 360px; overflow-wrap: break-word;";
        page.parentNode.insertBefore(tooltip, page);
    }

    displayFn(classname);
    displayTooltip(tooltip, classname);
}

function displayFn(classname) {
    let vis_num = document.getElementsByClassName(classname);
    let code_obj = vis_num[0];
    let tl_obj = vis_num[1];
    let c_svg = getSvgRoot(code_obj);
    let tl_svg = getSvgRoot(tl_obj);
    let triggers = tl_svg.getElementsByClassName('fn-trigger');
    var functions = c_svg.getElementsByClassName('fn');

    for (let i = 0; i < triggers.length; i++) {
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
    let tl_svg = getSvgRoot(tl_obj);
    let triggers = tl_svg.getElementsByClassName('tooltip-trigger');

    var time_start = null;

    for (let i = 0; i < triggers.length; i++) {
        if (triggers[i].classList.contains('listener')) break;
        else triggers[i].classList.add('listener');

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
        let code_root = getSvgRoot(code_obj);
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
                let emph = code_root.ownerDocument.createElementNS('http://www.w3.org/2000/svg', 'text');
                emph.setAttribute('class', 'code emph');
                emph.setAttribute('x', '25');
                emph.setAttribute('y', ly + 3);
                emph.innerHTML = new Array(
                    Math.floor(lines[i].getBBox().width/8)
                ).join('_');
                code_g.appendChild(emph);
            }
        }
    }
}


function mousePos(evt, obj) {
    let x_pos = evt.clientX + obj.getBoundingClientRect().x + 15;
    let y_pos = evt.clientY + obj.getBoundingClientRect().y + window.scrollY + 45;

    return {
        x: Math.round(x_pos),
        y: Math.round(y_pos)
    };
}

function removeUnderline(e, classname) {
    let code_obj = document.getElementsByClassName(classname + ' code_panel')[0];
    let code_root = getSvgRoot(code_obj);
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
