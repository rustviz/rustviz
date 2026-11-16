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
