// Make the RustViz panels follow the book's theme.
//
// mdBook marks the current theme with a class on <html> (light, rust,
// coal, navy or ayu) and swaps it when the reader picks another. The
// panels are SVG documents inside <object>s, out of reach of the page's
// CSS, but each carries a colour palette per theme (see the plugin's
// themes.rs) selected by `data-rv-theme` on its root <svg>. So: copy the
// theme onto every panel once it has loaded, and again whenever the
// theme class changes. A panel this script can't reach (the book opened
// from file://) keeps the light palette, or navy if the system prefers
// dark.
//
// Injected once per chapter, like helpers.js: guard against running twice.
(function () {
    if (window.__rvTheme) return;
    window.__rvTheme = true;
    var THEMES = ['light', 'rust', 'coal', 'navy', 'ayu'];

    function bookTheme() {
        var cl = document.documentElement.classList;
        for (var i = 0; i < THEMES.length; i++) {
            if (cl.contains(THEMES[i])) return THEMES[i];
        }
        return null;
    }

    function isPanel(el) {
        return el && el.tagName === 'OBJECT' &&
            (el.classList.contains('code_panel') || el.classList.contains('tl_panel'));
    }

    function themePanel(obj, theme) {
        var root;
        try {
            root = obj.contentDocument && obj.contentDocument.documentElement;
        } catch (e) {
            return; // not same-origin
        }
        // Before it loads, an <object>'s document is an empty HTML one.
        if (!root || root.nodeName.toLowerCase() !== 'svg') return;
        if (theme) {
            if (root.getAttribute('data-rv-theme') !== theme) root.setAttribute('data-rv-theme', theme);
        } else {
            root.removeAttribute('data-rv-theme');
        }
    }

    var current = bookTheme();
    function themeAll() {
        document.querySelectorAll('object.code_panel, object.tl_panel').forEach(function (o) {
            themePanel(o, current);
        });
    }

    // Panels that finish loading later. `load` doesn't bubble, but a
    // capturing listener on the document still sees it.
    document.addEventListener('load', function (e) {
        if (isPanel(e.target)) themePanel(e.target, bookTheme());
    }, true);

    // Theme switches. Only act when the theme itself changed: other code
    // (mdBook's sidebar toggle, Font Awesome's own observer) also writes
    // <html class>, and restyling on every write can feed a loop.
    new MutationObserver(function () {
        var t = bookTheme();
        if (t === current) return;
        current = t;
        themeAll();
    }).observe(document.documentElement, { attributes: true, attributeFilter: ['class'] });

    themeAll();
})();
