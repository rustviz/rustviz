//! Scraping SVG start tags. The plugin's SVG is regular enough that this
//! needs no XML parser; `xml_error` is the one strict parse, which checks
//! that a panel is well-formed XML (#1049).

/// `Some(message)` if `svg` is not well-formed XML. A browser shows an
/// error banner instead of the drawing when an SVG file opened on its own,
/// or embedded with `<object>`, is malformed (the playground's inline HTML
/// parse is lenient and hides it).
pub fn xml_error(svg: &str) -> Option<String> {
    roxmltree::Document::parse(svg).err().map(|e| e.to_string())
}

/// Value of attribute `name` in the start tag `tag` (`<circle cx=".." ..`),
/// matching only whole attribute names (`y` doesn't match inside `cy` or
/// `data-y`).
pub fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let pat = format!("{}=\"", name);
    let mut from = 0;
    while let Some(i) = tag[from..].find(&pat) {
        let at = from + i;
        let boundary = tag[..at]
            .chars()
            .next_back()
            .map_or(true, |c| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'));
        let rest = &tag[at + pat.len()..];
        if boundary {
            return rest.find('"').map(|end| &rest[..end]);
        }
        from = at + pat.len();
    }
    None
}

/// Whether the start tag `tag` is a move/copy/call arrow's shaft (a
/// `<polyline>`, or a hop arrow's `<path>`).
#[allow(dead_code)]
pub fn is_arrow_shaft(tag: &str) -> bool {
    attr(tag, "class").is_some_and(|c| c.split(' ').any(|c| c == "arrow-shaft"))
}

/// One element's `data-rv-*` attributes: the stable ids and kinds a
/// client (telemetry) names hovered elements by
/// (`rustviz-plugin/docs/element-ids.md`). Values are XML-decoded.
#[derive(Debug, Clone, PartialEq)]
pub struct RvElement {
    pub id: String,
    pub kind: String,
    pub event: Option<String>,
    pub state: Option<String>,
    pub var: Option<String>,
    pub to: Option<String>,
    pub line: Option<String>,
    pub line_end: Option<String>,
    pub part: Option<String>,
    pub drop: Option<String>,
}

/// The elements of a panel that carry `data-rv-id`, in document order.
pub fn rv_elements(svg: &str) -> Vec<RvElement> {
    let doc = roxmltree::Document::parse(svg).expect("well-formed SVG");
    doc.descendants()
        .filter(|n| n.attribute("data-rv-id").is_some())
        .map(|n| {
            let a = |k: &str| n.attribute(k).map(str::to_owned);
            RvElement {
                id: a("data-rv-id").unwrap(),
                kind: a("data-rv-kind").unwrap_or_default(),
                event: a("data-rv-event"),
                state: a("data-rv-state"),
                var: a("data-rv-var"),
                to: a("data-rv-to"),
                line: a("data-rv-line"),
                line_end: a("data-rv-line-end"),
                part: a("data-rv-part"),
                drop: a("data-rv-drop"),
            }
        })
        .collect()
}

/// The element kinds `data-rv-kind` may name.
pub const RV_KINDS: &[&str] = &[
    "header", "dot", "call", "fn", "arrow", "line", "branch_line", "stop_bar", "bracket", "code_line",
];

/// What's wrong with a panel's `data-rv-*` attributes: a hover target
/// (`tooltip-trigger`) or numbered code line without an id, an id used
/// twice, an unknown kind, an attribute its kind needs missing, or an
/// internal attribute the plugin should have rewritten, an id that isn't a
/// valid telemetry `element_id` (1-128 characters, no whitespace).
pub fn rv_problems(svg: &str) -> Vec<String> {
    let doc = match roxmltree::Document::parse(svg) {
        Ok(d) => d,
        Err(e) => return vec![format!("not well-formed: {}", e)],
    };
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for n in doc.descendants().filter(|n| n.is_element()) {
        let classes: Vec<&str> = n.attribute("class").unwrap_or("").split(' ').collect();
        let numbered_code = classes.contains(&"code")
            && n.descendants().any(|c| c.attribute("class") == Some("line-number")
                && c.text().is_some_and(|t| t.trim().chars().next().is_some_and(|c| c.is_ascii_digit())));
        let id = n.attribute("data-rv-id");
        if (classes.contains(&"tooltip-trigger") || numbered_code) && id.is_none() {
            out.push(format!("<{}> with tooltip {:?} has no data-rv-id",
                n.tag_name().name(), n.attribute("data-tooltip-text").unwrap_or("")));
        }
        for a in n.attributes() {
            if matches!(a.name(), "data-rv-row" | "data-rv-at") {
                out.push(format!("internal attribute {} left on {:?}", a.name(), id));
            }
        }
        let Some(id) = id else { continue };
        if !seen.insert(id.to_owned()) {
            out.push(format!("data-rv-id {:?} used twice", id));
        }
        // A telemetry `element_id`: 1-128 characters, no whitespace.
        if id.is_empty() || id.chars().count() > 128 || id.chars().any(char::is_whitespace) {
            out.push(format!("data-rv-id {:?} is empty, over 128 characters or has whitespace", id));
        }
        let kind = n.attribute("data-rv-kind").unwrap_or("");
        if !RV_KINDS.contains(&kind) {
            out.push(format!("{:?}: unknown data-rv-kind {:?}", id, kind));
        }
        let needs: &[&str] = match kind {
            "header" => &["data-rv-var"],
            "code_line" => &["data-rv-line"],
            "dot" | "call" | "fn" | "arrow" => &["data-rv-var", "data-rv-line", "data-rv-event"],
            "line" | "branch_line" | "bracket" => &["data-rv-var", "data-rv-line", "data-rv-state"],
            _ => &["data-rv-var", "data-rv-line"],
        };
        for a in needs {
            if n.attribute(*a).is_none() {
                out.push(format!("{:?} ({}) has no {}", id, kind, a));
            }
        }
        if matches!(kind, "arrow" | "fn") && n.attribute("data-rv-to").is_none() {
            out.push(format!("{:?} ({}) has no data-rv-to", id, kind));
        }
    }
    out
}
