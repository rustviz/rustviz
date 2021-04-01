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
            if (functions[i].getAttribute('hash') == evt_hash) {
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
        if (object.contentDocument.readyState == "complete") {
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

    for (let i = 0; i < triggers.length; i++) {
        // prevent adding duplicate listeners
        if (triggers[i].classList.contains('listener')) break;
        else triggers[i].classList.add('listener');

        triggers[i].addEventListener('mousemove', showTooltip);
        triggers[i].addEventListener('mouseleave', hideTooltip);
        triggers[i].addEventListener('mouseenter', insertCaret);
    }
    
    function showTooltip(e) {
        let mouse = mousePos(e, tl_obj);
        tooltip.style.transform = "translate(" + mouse.x + "px, " + mouse.y + "px)";
        tooltip.style.display = "block";
        
        let text = e.currentTarget.getAttributeNS(null, "data-tooltip-text");
        tooltip.innerHTML = text;

        // if out of bounds, break text into two lines
        if (tooltip.getBoundingClientRect().right >= document.body.clientWidth) breakText(text, tooltip);
    }

    function hideTooltip(e) {
        tooltip.style.display = "none";
        tooltip.innerHTML = '';

        removeCaret(e, classname);
    }

    /* ---- SHOW RELEVANT LINES ---- */
    function insertCaret(e) {
        let doc = document.getElementsByClassName(classname + ' code_panel')[0].contentDocument; //code_panelcc
        if (e.currentTarget.tagName == 'path') {
            let arr = e.currentTarget.getAttribute('d').split(' ');
            let begin, end;
            if (e.currentTarget.parentNode.id == 'ref_line') {
                begin = arr[2];
                end = parseInt(begin) + 2*parseInt(arr[5]) + parseInt(arr[7]) + 5; // + 5 to include last line
            }
            else {
                begin = arr[7];
                end = arr[3];
            }
            for (let i=0; i < (end-begin)/20; ++i) {
                let caret = doc.createElementNS('http://www.w3.org/2000/svg', 'text');
                caret.setAttribute('class', 'code caret');
                caret.setAttribute('x', '15');
                caret.setAttribute('y', parseInt(begin) + 20*i + 5);
                caret.innerHTML = '>';
                doc.getElementById('code').appendChild(caret);
            }
        }
        else if (e.currentTarget.tagName == 'line') {
            let begin = e.currentTarget.getAttribute('y1');
            let end = e.currentTarget.getAttribute('y2');
            for (let i=0; i < (end-begin)/20; ++i) {
                let caret = doc.createElementNS('http://www.w3.org/2000/svg', 'text');
                caret.setAttribute('class', 'code caret');
                caret.setAttribute('x', '15');
                caret.setAttribute('y', parseInt(begin) + 20*i + 5);
                caret.innerHTML = '>';
                doc.getElementById('code').appendChild(caret);
            }
        }
        else {
            let pos;
            if (e.currentTarget.tagName == 'circle') {
                pos = parseInt(e.currentTarget.getAttribute('cy')) + 5;
            }
            else if (e.currentTarget.tagName == 'use') {
                pos = parseInt(e.currentTarget.getAttribute('y')) + 5;
            }
            else if (e.currentTarget.tagName == 'polyline') {
                var arr = e.currentTarget.getAttribute('points').split(' ');
                pos = parseInt(arr[1]) + 5;
            }
            else { // e.currentTarget.tagName == 'text'
                pos = parseInt(e.currentTarget.getAttribute('y'));
            }

            let caret = doc.createElementNS('http://www.w3.org/2000/svg', 'text');
            caret.setAttribute('class', 'code caret');
            caret.setAttribute('x', '15');
            caret.setAttribute('y', pos);
            caret.innerHTML = '>';
            doc.getElementById('code').appendChild(caret);
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

function removeCaret(e, classname) {
    let doc = document.getElementsByClassName(classname + ' code_panel')[0].contentDocument; //code_panel
    let arr = doc.getElementsByClassName('caret');
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

/*window.onload = function () {
    let correct_doc = (document.getElementsByClassName('active')[0].attributes.href.value == 'ch04-01-what-is-ownership.html'
            || document.getElementsByClassName('active')[0].attributes.href.value == 'ch04-02-references-and-borrowing.html');

    if (correct_doc) {
        let top_btns = document.getElementsByClassName('left-buttons');

        let eye = document.getElementById('viz-toggle');

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