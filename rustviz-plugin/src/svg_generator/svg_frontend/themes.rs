//! Colour themes for the two SVG panels.
//!
//! Every colour the panels draw with is a CSS custom property
//! (`--rv-bg`, `--rv-code`, ..., `--rv-c1`..`--rv-c12` for the
//! columns). Each theme gives all of them a value; the stylesheet
//! (`theme_css`) declares the default theme on `:root` and the others
//! under `[data-rv-theme="<name>"]`, so a page picks one by setting
//! that attribute on the panel's root `<svg>` (an `<object>`-embedded
//! panel, as in mdbook-rustviz) or on any ancestor of an inline one
//! (the playground, `rustviz html`). With no attribute the panel is
//! `light`, or `navy` when the reader's system prefers a dark scheme.
//!
//! The themes match mdBook's five built-in ones. Backgrounds and text
//! colours come from mdBook 0.5's `variables.css` and its code-block
//! highlighters (`highlight.css` for light/rust, `tomorrow-night.css`
//! for coal/navy, `ayu-highlight.css` for ayu), so a panel looks like
//! a code block of the book around it. `light` is RustViz's original
//! palette, unchanged.

/// The value of every themed colour in one theme.
pub struct Theme {
    /// The `data-rv-theme` value (and mdBook's theme class).
    pub name: &'static str,
    /// Panel background (also the fill of the hollow `f` call icons).
    pub bg: &'static str,
    /// Code text, keywords and function names.
    pub code: &'static str,
    pub comment: &'static str,
    pub string: &'static str,
    pub line_number: &'static str,
    /// The "Hover over timeline events..." caption.
    pub caption: &'static str,
    /// Move/copy/call arrows.
    pub arrow: &'static str,
    /// The small triangle inside a drop dot.
    pub drop_notch: &'static str,
    /// A struct's grouping box, drawn at 10% opacity.
    pub struct_fill: &'static str,
    pub struct_stroke: &'static str,
    /// The line under the code a hovered timeline element refers to.
    pub underline: &'static str,
    /// The tooltip lives in the host page; helpers.js reads these three
    /// from the timeline panel so it follows the panel's theme.
    pub tooltip_bg: &'static str,
    pub tooltip_fg: &'static str,
    pub tooltip_border: &'static str,
    /// Column colours: column hash `h >= 1` uses `columns[(h - 1) % 12]`.
    pub columns: [&'static str; 12],
}

impl Theme {
    /// `(custom property, value)` for every colour, in a fixed order.
    pub fn vars(&self) -> Vec<(String, &'static str)> {
        let mut v: Vec<(String, &'static str)> = [
            ("bg", self.bg),
            ("code", self.code),
            ("comment", self.comment),
            ("string", self.string),
            ("line-number", self.line_number),
            ("caption", self.caption),
            ("arrow", self.arrow),
            ("drop-notch", self.drop_notch),
            ("struct-fill", self.struct_fill),
            ("struct-stroke", self.struct_stroke),
            ("underline", self.underline),
            ("tooltip-bg", self.tooltip_bg),
            ("tooltip-fg", self.tooltip_fg),
            ("tooltip-border", self.tooltip_border),
        ]
        .into_iter()
        .map(|(k, c)| (format!("--rv-{k}"), c))
        .collect();
        for (i, c) in self.columns.iter().enumerate() {
            v.push((format!("--rv-c{}", i + 1), c));
        }
        v
    }
}

/// RustViz's original colours. The default when no theme is set.
pub const LIGHT: Theme = Theme {
    name: "light",
    bg: "#f1f1f1",
    code: "#6e6b5e",
    comment: "#8a8a8a",
    string: "#3a6bb5",
    line_number: "#aaaaaa",
    caption: "#000000",
    arrow: "#808080",
    drop_notch: "#ffffff",
    struct_fill: "#ffffff",
    struct_stroke: "#000000",
    // The page's text colour, as before themes: black in an <object>,
    // the surrounding text colour when inlined.
    underline: "currentColor",
    tooltip_bg: "rgba(70, 70, 70, 0.6)",
    tooltip_fg: "#ffffff",
    tooltip_border: "transparent",
    columns: [
        "#1893ff", "#ff7f50", "#8635ff", "#dc143c", "#0a810a", "#008080",
        "#ff6cce", "#00d6fc", "#b99f35", "#15a710", "#000000", "#463ae9",
    ],
};

/// mdBook "rust": the light code-block background, the theme's own
/// code colour (`--inline-code-color`, which is also RustViz's
/// original code colour), and the original column hues darkened until
/// every column line and dot has at least 3:1 contrast.
pub const RUST: Theme = Theme {
    name: "rust",
    bg: "#f6f7f6",
    code: "#6e6b5e",
    comment: "#6b6b66",
    string: "#2f5f9e",
    line_number: "#6f6f68",
    caption: "#262625",
    arrow: "#767676",
    drop_notch: "#ffffff",
    struct_fill: "#ffffff",
    struct_stroke: "#000000",
    underline: "#262625",
    tooltip_bg: "#e1e1db",
    tooltip_fg: "#262625",
    tooltip_border: "#b38f6b",
    columns: [
        "#1a6fd6", "#d4551f", "#8635ff", "#c8102e", "#0a7d0a", "#007f7f",
        "#c93a9f", "#0090b0", "#a06f00", "#5c8f00", "#000000", "#463ae9",
    ],
};

/// Column colours for the dark themes: the original hues, lightened
/// until every one has at least 4.5:1 contrast as text on the dark
/// code-block backgrounds. Black becomes near-white; the two greens
/// and the two purples are pushed apart (lime vs green, violet vs
/// periwinkle), since lightening alone made them hard to tell apart.
const DARK_COLUMNS: [&str; 12] = [
    "#4aa8ff", "#ff8c61", "#c084fc", "#f0475e", "#4cc35a", "#2ec4b6",
    "#ff7ad9", "#3fdcff", "#d4b94a", "#a6e22e", "#e8e8e8", "#8c8cff",
];

/// mdBook "coal": tomorrow-night code blocks, coal's popup colours.
pub const COAL: Theme = Theme {
    name: "coal",
    bg: "#1d1f21",
    code: "#c5c8c6",
    comment: "#969896",
    string: "#81a2be",
    line_number: "#868a8e",
    caption: "#98a3ad",
    arrow: "#8e9399",
    drop_notch: "#1d1f21",
    struct_fill: "#000000",
    struct_stroke: "#ffffff",
    underline: "#c5c8c6",
    tooltip_bg: "#141617",
    tooltip_fg: "#98a3ad",
    tooltip_border: "#43484d",
    columns: DARK_COLUMNS,
};

/// mdBook "navy": the same tomorrow-night code blocks as coal (mdBook
/// uses one highlighter for both), navy's text and popup colours.
pub const NAVY: Theme = Theme {
    name: "navy",
    bg: "#1d1f21",
    code: "#c5c8c6",
    comment: "#969896",
    string: "#81a2be",
    line_number: "#868a8e",
    caption: "#bcbdd0",
    arrow: "#8e9399",
    drop_notch: "#1d1f21",
    struct_fill: "#000000",
    struct_stroke: "#ffffff",
    underline: "#c5c8c6",
    tooltip_bg: "#161923",
    tooltip_fg: "#bcbdd0",
    tooltip_border: "#737480",
    columns: DARK_COLUMNS,
};

/// mdBook "ayu": ayu-highlight code blocks. Ayu's own comment colour
/// (#5c6773) is only 3:1 on its background, so comments use a lighter
/// shade of it.
pub const AYU: Theme = Theme {
    name: "ayu",
    bg: "#191f26",
    code: "#e6e1cf",
    comment: "#8a96a3",
    string: "#81a2be",
    line_number: "#818c98",
    caption: "#c5c5c5",
    arrow: "#8e959e",
    drop_notch: "#191f26",
    struct_fill: "#000000",
    struct_stroke: "#ffffff",
    underline: "#e6e1cf",
    tooltip_bg: "#14191f",
    tooltip_fg: "#c5c5c5",
    tooltip_border: "#5c6773",
    columns: DARK_COLUMNS,
};

pub const THEMES: [&Theme; 5] = [&LIGHT, &RUST, &COAL, &NAVY, &AYU];

/// The theme used when none is set...
pub const DEFAULT_THEME: &Theme = &LIGHT;
/// ...unless the reader's system prefers a dark colour scheme (mdBook's
/// default `preferred-dark-theme`).
pub const DEFAULT_DARK_THEME: &Theme = &NAVY;

fn declarations(t: &Theme, indent: &str) -> String {
    t.vars()
        .into_iter()
        .map(|(k, v)| format!("{indent}{k}: {v};\n"))
        .collect()
}

/// The custom-property declarations of every theme. Must come before
/// any rule that reads them (it does: it opens the stylesheet).
pub fn theme_css() -> String {
    let mut css = String::from(
        "/* colour themes: every colour below is one of these properties.\n       \
         Pick a theme with data-rv-theme=\"light|rust|coal|navy|ayu\" on\n       \
         the root <svg> or, for an inline <svg>, on any ancestor. */\n",
    );
    // The default theme's block doubles as the `:root` default.
    css.push_str(&format!("    :root,\n    [data-rv-theme=\"{}\"] {{\n", DEFAULT_THEME.name));
    css.push_str(&declarations(DEFAULT_THEME, "        "));
    css.push_str("    }\n");
    // Matches no element that has (or, inline, sits under) an explicit
    // theme: that element's own declarations win over inherited ones.
    css.push_str("    @media (prefers-color-scheme: dark) {\n        :root:not([data-rv-theme]) {\n");
    css.push_str(&declarations(DEFAULT_DARK_THEME, "            "));
    css.push_str("        }\n    }\n");
    for t in THEMES.iter().filter(|t| !std::ptr::eq(**t, DEFAULT_THEME)) {
        css.push_str(&format!("    [data-rv-theme=\"{}\"] {{\n", t.name));
        css.push_str(&declarations(t, "        "));
        css.push_str("    }\n");
    }
    css
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgb(c: &str) -> (f64, f64, f64) {
        let h = c.strip_prefix('#').unwrap_or_else(|| panic!("{c} is not #rrggbb"));
        assert_eq!(h.len(), 6, "{c} is not #rrggbb");
        let p = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).unwrap() as f64 / 255.0;
        (p(0), p(2), p(4))
    }

    fn luminance(c: &str) -> f64 {
        let lin = |v: f64| if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
        let (r, g, b) = rgb(c);
        0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
    }

    /// WCAG 2 contrast ratio.
    fn contrast(a: &str, b: &str) -> f64 {
        let (x, y) = (luminance(a), luminance(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    #[test]
    fn light_is_the_original_palette() {
        // The default theme must render exactly as RustViz did before
        // themes; these are the colours main hard-coded.
        let l = &LIGHT;
        assert_eq!(
            (l.bg, l.code, l.comment, l.string, l.line_number, l.caption, l.arrow),
            ("#f1f1f1", "#6e6b5e", "#8a8a8a", "#3a6bb5", "#aaaaaa", "#000000", "#808080"),
        );
        assert_eq!((l.drop_notch, l.struct_fill, l.struct_stroke), ("#ffffff", "#ffffff", "#000000"));
        assert_eq!((l.tooltip_bg, l.tooltip_fg), ("rgba(70, 70, 70, 0.6)", "#ffffff"));
        assert_eq!(
            l.columns,
            [
                "#1893ff", "#ff7f50", "#8635ff", "#dc143c", "#0a810a", "#008080",
                "#ff6cce", "#00d6fc", "#b99f35", "#15a710", "#000000", "#463ae9",
            ]
        );
        assert!(std::ptr::eq(DEFAULT_THEME, &LIGHT));
    }

    #[test]
    fn themed_palettes_have_enough_contrast() {
        // Every theme but `light` (kept as it was) meets WCAG AA for
        // text (4.5:1) and 3:1 for lines, dots and arrows. Column
        // colours are both: lines and dots, and the column labels and
        // variable names in the code. Dark themes reach 4.5:1 for all
        // twelve; on the light rust background twelve distinct hues
        // can't all be that dark, so there they only need 3:1.
        for t in THEMES.iter().filter(|t| t.name != "light") {
            for (role, c) in [
                ("code", t.code),
                ("comment", t.comment),
                ("string", t.string),
                ("line number", t.line_number),
                ("caption", t.caption),
            ] {
                let r = contrast(c, t.bg);
                assert!(r >= 4.5, "{}: {role} {c} is {r:.2}:1 on {}", t.name, t.bg);
            }
            let r = contrast(t.tooltip_fg, t.tooltip_bg);
            assert!(r >= 4.5, "{}: tooltip text is {r:.2}:1", t.name);
            let r = contrast(t.arrow, t.bg);
            assert!(r >= 3.0, "{}: arrow {} is {r:.2}:1", t.name, t.arrow);
            let dark = luminance(t.bg) < 0.2;
            for (i, c) in t.columns.iter().enumerate() {
                let r = contrast(c, t.bg);
                let min = if dark { 4.5 } else { 3.0 };
                assert!(r >= min, "{}: column {} {c} is {r:.2}:1 on {}", t.name, i + 1, t.bg);
            }
        }
    }

    #[test]
    fn every_theme_declares_every_property() {
        let css = theme_css();
        let names: Vec<String> = LIGHT.vars().into_iter().map(|(k, _)| k).collect();
        assert_eq!(names.len(), 14 + 12);
        for t in THEMES {
            let block = css
                .split(&format!("[data-rv-theme=\"{}\"] {{", t.name))
                .nth(1)
                .unwrap_or_else(|| panic!("no block for {}", t.name));
            let block = &block[..block.find('}').unwrap()];
            for n in &names {
                assert!(block.contains(&format!("{n}: ")), "{} lacks {n}", t.name);
            }
        }
        // light is also the default; the system-dark fallback only
        // applies when no theme is set
        assert!(css.contains(":root,\n    [data-rv-theme=\"light\"] {"));
        assert!(css.contains(
            "@media (prefers-color-scheme: dark) {\n        :root:not([data-rv-theme]) {"
        ));
    }
}
