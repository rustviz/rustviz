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

/* --------------------- SIMPLE DRIVER --------------------- */
function helpers(classname) {
    // create tooltip element before #page-wrapper
    let page = document.querySelector('#page-wrapper');
    let tooltip = document.getElementById('svg_tooltip');
    if (!tooltip) {
        tooltip = document.createElement('p');
        tooltip.id = 'svg_tooltip';
        tooltip.style.cssText = "position: absolute; padding: 0.5em; font-size: 0.75em; border-radius: 8px;" +
                                "font-family: 'Trebuchet MS', Helvetica, sans-serif;" +
                                "background: rgb(70, 70, 70, 0.6); color: white; z-index: 100; display: none;";
        page.parentNode.insertBefore(tooltip, page);
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
    let c_svg = code_obj.contentDocument.firstChild;
    let tl_svg = tl_obj.contentDocument.firstChild
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
    let tl_obj = document.getElementsByClassName(classname)[1];
    let tl_svg = tl_obj.contentDocument.firstChild
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

        // only track hovering after mouse leaves element
        gtag('event', 'tooltip_hover', {
            'event_label': e_label,
        });

        gtag('event', e_label, {
            'hover_time': (Date.now() - time_start) // time in ms
        });
        time_start = null; // reset

        removeUnderline(e, classname);
    }

    /* ---- SHOW RELEVANT LINES ---- */
    function insertUnderline(e) {
        let doc = document.getElementsByClassName(classname + ' code_panel')[0].contentDocument; //code_panel
        let begin = 0, end = 0;
        if (e.currentTarget.tagName === 'path') {
            let arr = e.currentTarget.getAttribute('d').split(' ');
            if (e.currentTarget.parentNode.id === 'ref_line') {
                begin = parseInt(arr[2]);
                end = parseInt(begin) + 2*parseInt(arr[5]) + parseInt(arr[7]) + 5; // + 5 to include last line
            }
            else {
                begin = parseInt(arr[7]);
                end = parseInt(arr[3]);
            }
        }
        else if (e.currentTarget.tagName === 'line') {
            begin = e.currentTarget.getAttribute('y1');
            end = e.currentTarget.getAttribute('y2');
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
            else { // e.currentTarget.tagName === 'text'
                begin = end = parseInt(e.currentTarget.getAttribute('y'));
            }
        }

        // add underlining to every relevant line
        let lines = doc.getElementById('code').children;
        let len = lines.length; // prevent len from changing
        for (let i=0; i<len; ++i) {
            let ly = parseInt(lines[i].getAttribute('y'));
            if (ly >= begin && ly <= end) { // only underline relevant code
                let emph = doc.createElementNS('http://www.w3.org/2000/svg', 'text');
                emph.setAttribute('class', 'code emph');
                emph.setAttribute('x', '25');
                emph.setAttribute('y', ly + 3); // +3 to hang just under text
                emph.innerHTML = new Array(
                    Math.floor(lines[i].getBBox().width/8) // size of '_' = 8
                ).join('_'); // string with all underscores
                doc.getElementById('code').appendChild(emph);
            }
        }
    }
}

// track mouse movement
function mousePos(evt, obj) {
    let x_pos = evt.clientX + obj.getBoundingClientRect().x + 15; // offset from svg start + svg offset
    let y_pos = evt.clientY + obj.getBoundingClientRect().y + window.scrollY + 45; // baseline hanging

    return {
        //object
        x: Math.round(x_pos),
        y: Math.round(y_pos)
    };
}

function removeUnderline(e, classname) {
    let doc = document.getElementsByClassName(classname + ' code_panel')[0].contentDocument; //code_panel
    let arr = doc.getElementsByClassName('emph');
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

/*window.onload = function () {
    var correct_doc = (document.getElementsByClassName('active')[0].attributes.href.value === 'ch04-01-what-is-ownership.html'
            || document.getElementsByClassName('active')[0].attributes.href.value === 'ch04-02-references-and-borrowing.html');

    if (correct_doc) {
        let top_btns = document.getElementsByClassName('left-buttons');

        var eye = document.getElementById('viz-toggle');
        var struct_eye = document.getElementById('viz-struct-toggle')

        if (!eye) {
            eye = document.createElement('button');
            eye.id = 'viz-toggle';
            eye.className = 'icon-button fa fa-eye';
            eye.title = 'Toggle all visualizations';
            top_btns[0].insertBefore(eye, top_btns[0].lastElementChild);
        }

        eye.addEventListener('click', function (e) {
            if (e.currentTarget.classList.contains('fa-eye')) {
                // on button click, show all visualizations
                e.currentTarget.classList.remove('fa-eye');
                e.currentTarget.classList.add('fa-eye-slash');

                toggleAll(true);
            } else if (e.currentTarget.classList.contains('fa-eye-slash')) {
                // on button click, hide all visualizations
                e.currentTarget.classList.remove('fa-eye-slash');
                e.currentTarget.classList.add('fa-eye');

                toggleAll(false);
            }
        });
    }
};*/
