//! Scraping SVG start tags. The plugin's SVG is regular enough that this
//! saves pulling an XML parser into the dev-dependencies.

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
