/* --------------------- SIMPLE DRIVER --------------------- */
function helpers(classname) {
    // create tooltip element before #page-wrapper
    var page = document.querySelector('#page-wrapper');
    var tooltip = document.getElementById('svg_tooltip');
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
    var vis_num = document.getElementsByClassName(classname);
    var code_obj = vis_num[0];
    var tl_obj = vis_num[1];
    var c_svg = code_obj.contentDocument.firstChild;
    var tl_svg = tl_obj.contentDocument.firstChild
    // get elements that will trigger function
    var triggers = tl_svg.getElementsByClassName('fn-trigger');
    var functions = c_svg.getElementsByClassName('fn');
    
    for (var i = 0; i < triggers.length; i++) {
        triggers[i].addEventListener('mouseover', showFn);
        triggers[i].addEventListener('mouseout', hideFn);
    }
    
    function showFn(evt) {
        // get target attributes
        var evt_hash = evt.target.dataset.hash;
        // var color = evt.target.style.fill;

        for (var i = 0; i < functions.length; i++) {
            // if hashes match, temporarily change color
            if (functions[i].getAttribute('hash') == evt_hash) {
                functions[i].dataset.hash = evt_hash;
            }
        }
    }

    function hideFn() {
        // reset to hash 0, styling to black on mouseout
        for (var i = 0; i < functions.length; i++) {
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
            var svg_doc = object.contentDocument;
            var code_width = svg_doc.getElementById('code').getBBox().width;
            var new_width = Math.max(code_width + 30, 400);
            svg_doc.firstChild.setAttribute('width', new_width + 'px');
        }, {once: true});
    }
    else {
        if (object.contentDocument.readyState == "complete") {
            var svg_doc = object.contentDocument;
            var code_width = svg_doc.getElementById('code').getBBox().width;
            var new_width = Math.max(code_width + 30, 400);
            svg_doc.firstChild.setAttribute('width', new_width + 'px');
        }
    }
}

/* --------------------- TOOLTIP-RELATED FUNCTIONS --------------------- */

// change tooltip text on hover
function displayTooltip(tooltip, classname) {
    // get svg elements
    var tl_obj = document.getElementsByClassName(classname)[1];
    var tl_svg = tl_obj.contentDocument.firstChild
    // get elements that will trigger function
    var triggers = tl_svg.getElementsByClassName('tooltip-trigger');

    for (var i = 0; i < triggers.length; i++) {
        // prevent adding duplicate listeners
        if (triggers[i].classList.contains('listener')) break;
        else triggers[i].classList.add('listener');

        triggers[i].addEventListener('mousemove', showTooltip);
        triggers[i].addEventListener('mouseleave', hideTooltip);
        triggers[i].addEventListener('mouseenter', insertCaret);
    }
    
    function showTooltip(e) {
        var mouse = mousePos(e, tl_obj);
        tooltip.style.transform = "translate(" + mouse.x + "px, " + mouse.y + "px)";
        tooltip.style.display = "block";

        // var text = e.target.getAttribute("data-tooltip-text");
        var text = e.target.getAttributeNS(null, "data-tooltip-text");
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
        var doc = document.getElementsByClassName(classname + ' code_panel')[0].contentDocument; //code_panelcc
        if (e.target.tagName == 'path') {
            var arr = e.target.getAttribute('d').split(' ');
            var begin, end;
            if (e.target.parentNode.id == 'ref_line') {
                begin = arr[2];
                end = parseInt(begin) + 2*parseInt(arr[5]) + parseInt(arr[7]) + 5; // + 5 to include last line
            }
            else {
                begin = arr[7];
                end = arr[3];
            }
            for (var i=0; i < (end-begin)/20; ++i) {
                var caret = doc.createElementNS('http://www.w3.org/2000/svg', 'text');
                caret.setAttribute('class', 'code caret');
                caret.setAttribute('x', '15');
                caret.setAttribute('y', parseInt(begin) + 20*i + 5);
                caret.innerHTML = '>';
                doc.getElementById('code').appendChild(caret);
            }
        }
        else if (e.target.tagName == 'line') {
            var begin = e.target.getAttribute('y1');
            var end = e.target.getAttribute('y2');
            for (var i=0; i < (end-begin)/20; ++i) {
                var caret = doc.createElementNS('http://www.w3.org/2000/svg', 'text');
                caret.setAttribute('class', 'code caret');
                caret.setAttribute('x', '15');
                caret.setAttribute('y', parseInt(begin) + 20*i + 5);
                caret.innerHTML = '>';
                doc.getElementById('code').appendChild(caret);
            }
        }
        else {
            var pos;
            if (e.target.tagName == 'circle') {
                pos = parseInt(e.target.getAttribute('cy')) + 5;
            }
            else if (e.target.tagName == 'use') {
                pos = parseInt(e.target.getAttribute('y')) + 5;
            }
            else if (e.target.tagName == 'polyline') {
                var arr = e.target.getAttribute('points').split(' ');
                pos = parseInt(arr[1]) + 5;
            }
            else { // e.target.tagName == 'text'
                pos = parseInt(e.target.getAttribute('y'));
            }

            var caret = doc.createElementNS('http://www.w3.org/2000/svg', 'text');
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
    var x_pos = evt.clientX + obj.getBoundingClientRect().x + 15; // offset from svg start + svg offset
    var y_pos = evt.clientY + obj.getBoundingClientRect().y + window.scrollY + 45; // baseline hanging

    return {
        //object
        x: Math.round(x_pos),
        y: Math.round(y_pos)
    };
}

function removeCaret(e, classname) {
    var doc = document.getElementsByClassName(classname + ' code_panel')[0].contentDocument; //code_panel
    var arr = doc.getElementsByClassName('caret');
    for (i = arr.length-1; i >= 0; --i) {
        arr[i].remove();
    }
}

// adjust text box
function breakText(text, tooltip) {
    // combine span into one element
    var split_text = text.split(' ');
    var words = [];
    var last = 0, span = false;
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
    var left = tooltip.getBoundingClientRect().left;
    for (const word of words) {
        tooltip.innerHTML += (word + ' ');
        if (left + tooltip.clientWidth > document.body.clientWidth - 20) {
            // reset tooltip text and break into new lines
            var idx = tooltip.innerHTML.lastIndexOf(' ', tooltip.innerHTML.length-2);
            var temp = tooltip.innerHTML.substr(0, idx);
            var other = tooltip.innerHTML.substr(idx + 1);

            tooltip.innerHTML = '';
            tooltip.innerHTML += temp;
            tooltip.innerHTML += ('<br />' + other);
        }
    }
}

/* --------------- TOGGLE ALL SVGS --------------- */
function toggleAll(turn_on) {
    var evt = new MouseEvent("click", {
      bubbles: true,
      cancelable: true,
      view: window
    });

    var arr = document.getElementsByClassName('toggle-button');
    for (const obj of arr) {
        if (turn_on && obj.classList.contains('fa-toggle-off')) {
            obj.dispatchEvent(evt);
        }
        else if (!turn_on && obj.classList.contains('fa-toggle-on')) {
            obj.dispatchEvent(evt);
        }
    }
}

/* --------------- TOGGLE ALL SVGS --------------- */
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

window.onload = function () {
    var correct_doc = (document.getElementsByClassName('active')[0].attributes.href.value == 'ch04-01-what-is-ownership.html'
            || document.getElementsByClassName('active')[0].attributes.href.value == 'ch04-02-references-and-borrowing.html');

    if (correct_doc) {
        var top_btns = document.getElementsByClassName('left-buttons');

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
            if (e.target.classList.contains('fa-eye')) {
                // on button click, show all visualizations
                e.target.classList.remove('fa-eye');
                e.target.classList.add('fa-eye-slash');

                toggleAll(true);
            } else if (e.target.classList.contains('fa-eye-slash')) {
                // on button click, hide all visualizations
                e.target.classList.remove('fa-eye-slash');
                e.target.classList.add('fa-eye');

                toggleAll(false);
            }
        });
    }
};