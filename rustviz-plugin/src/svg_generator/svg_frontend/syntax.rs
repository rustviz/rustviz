//! Lightweight Rust syntax highlighter for the code panel.
//!
//! The plugin's annotation pass injects `<tspan data-hash="…">name</tspan>`
//! wrappers around variable identifiers so the renderer can color them
//! and the helpers JS can highlight them on hover. By the time the
//! code panel emits a line, those wrappers are already present and the
//! rest of the line is HTML-escaped (`&amp;`, `&lt;`, `&gt;`).
//!
//! [`highlight`] walks one such already-annotated line, treats
//! `<tspan …>…</tspan>` regions as opaque (passes them through
//! verbatim), and wraps three additional token kinds in new `<tspan>`s:
//!
//! - **Keywords** (`let`, `fn`, `mut`, …) get `class="kw"` (rendered
//!   bold by the templates' CSS).
//! - **Line comments** (`// …`) and **block comments** (`/* … */`,
//!   including ones that span multiple lines) get `class="comment"`
//!   (dark gray italic).
//! - **String literals** (`"…"`, `r"…"`, `b"…"`, `r#"…"#`, …) get
//!   `class="string"` (navy). Raw / byte / C prefixes are folded into
//!   the same span so the prefix character is colored too.
//!
//! Block-comment state crosses line boundaries via the `in_block_comment`
//! flag — callers thread one mutable bool through their per-line loop.
//! Nested block comments (`/* outer /* inner */ outer */`) close on the
//! first `*/`; this is the only documented Rust grammar deviation.
//! Number / char literals aren't styled (intentionally — too noisy on
//! teaching snippets that are mostly numeric examples).

/// Reserved words plus contextual keywords. Picked to match what
/// `rustc_lexer` treats as a keyword token, minus the truly obscure
/// reserved-but-unused identifiers — dropping `become`, `priv`, etc.
/// keeps the bold-set close to what users actually see in modern Rust.
const KEYWORDS: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn",
    "else", "enum", "extern", "false", "fn", "for", "if", "impl", "in",
    "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return",
    "Self", "self", "static", "struct", "super", "trait", "true", "type",
    "union", "unsafe", "use", "where", "while",
];

/// Wrap Rust keywords / comments / string literals in classed `<tspan>`s,
/// leaving the line's existing `<tspan data-hash="…">` and
/// `<tspan class="fn" …>` wrappers untouched.
///
/// `in_block_comment` is the running state of an open `/* … */` from a
/// previous line. Set it to `false` for the first line and pass the same
/// `&mut bool` to every subsequent call so a multi-line block comment
/// stays styled across the whole span.
pub fn highlight(line: &str, in_block_comment: &mut bool) -> String {
    let bytes = line.as_bytes();
    let mut out = String::with_capacity(line.len() + 32);
    let mut i = 0;

    // We entered this line still inside a `/* … */` from above. Emit
    // everything up to (and including) the closing `*/` — or the entire
    // line, if it doesn't close here — as a comment span.
    if *in_block_comment {
        out.push_str("<tspan class=\"comment\">");
        if let Some(end_off) = line.find("*/") {
            let end = end_off + 2;
            out.push_str(&line[..end]);
            out.push_str("</tspan>");
            *in_block_comment = false;
            i = end;
        } else {
            out.push_str(line);
            out.push_str("</tspan>");
            return out;
        }
    }

    while i < bytes.len() {
        // Pass annotation tspans through verbatim. The plugin only
        // emits flat `<tspan …>name</tspan>` (no nesting), so a
        // forward scan to the matching `</tspan>` is safe.
        if line[i..].starts_with("<tspan") {
            if let Some(end) = find_close_tspan(&line[i..]) {
                out.push_str(&line[i..i + end]);
                i += end;
                continue;
            }
            // Malformed tag — bail, emit rest as-is.
            out.push_str(&line[i..]);
            return out;
        }

        // Line comment: // through end of line.
        if line[i..].starts_with("//") {
            out.push_str("<tspan class=\"comment\">");
            out.push_str(&line[i..]);
            out.push_str("</tspan>");
            return out;
        }

        // Block comment: /* through */, possibly spanning lines.
        if line[i..].starts_with("/*") {
            if let Some(end_off) = line[i + 2..].find("*/") {
                let end = i + 2 + end_off + 2;
                out.push_str("<tspan class=\"comment\">");
                out.push_str(&line[i..end]);
                out.push_str("</tspan>");
                i = end;
                continue;
            }
            // Unclosed on this line — emit the rest as a comment span
            // and flag the next call so it picks up where we left off.
            out.push_str("<tspan class=\"comment\">");
            out.push_str(&line[i..]);
            out.push_str("</tspan>");
            *in_block_comment = true;
            return out;
        }

        // Bare string literal (`"…"`). Backslash escapes are honored
        // so a quote inside the string doesn't terminate it early.
        if bytes[i] == b'"' {
            let end = scan_string_body(bytes, i, /*is_raw=*/ false);
            out.push_str("<tspan class=\"string\">");
            out.push_str(&line[i..end]);
            out.push_str("</tspan>");
            i = end;
            continue;
        }

        // Identifier — possibly a keyword, or possibly the prefix of a
        // raw / byte / C string literal (`r"…"`, `b"…"`, `br"…"`,
        // `r#"…"#`, …). Check the closed prefix set first so we color
        // the prefix character as part of the string.
        if is_ident_start(bytes[i]) {
            let mut j = i + 1;
            while j < bytes.len() && is_ident_continue(bytes[j]) {
                j += 1;
            }
            let word = &line[i..j];

            if matches!(word, "r" | "b" | "c" | "br" | "rb" | "cr")
                && j < bytes.len()
                && (bytes[j] == b'"' || bytes[j] == b'#')
            {
                let is_raw = word.contains('r');
                if let Some(end) = try_prefixed_string(bytes, j, is_raw) {
                    out.push_str("<tspan class=\"string\">");
                    out.push_str(&line[i..end]);
                    out.push_str("</tspan>");
                    i = end;
                    continue;
                }
            }

            if KEYWORDS.contains(&word) {
                out.push_str("<tspan class=\"kw\">");
                out.push_str(word);
                out.push_str("</tspan>");
            } else {
                out.push_str(word);
            }
            i = j;
            continue;
        }

        // Anything else — emit one char and advance.
        let ch = line[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }

    out
}

/// Scan a non-raw string body starting at `start` (which points at the
/// opening `"`). Honors `\"` and `\\` escapes. On an unterminated
/// literal returns the end of the slice — the highlighter then just
/// styles the whole tail of the line as a string, which is the least
/// surprising fallback for a teaching snippet that's mid-edit.
fn scan_string_body(bytes: &[u8], start: usize, is_raw: bool) -> usize {
    debug_assert_eq!(bytes[start], b'"');
    let mut j = start + 1;
    while j < bytes.len() {
        if !is_raw && bytes[j] == b'\\' && j + 1 < bytes.len() {
            j += 2;
            continue;
        }
        if bytes[j] == b'"' {
            return j + 1;
        }
        j += 1;
    }
    j
}

/// Scan a raw-string body that may have leading `#`s, starting at the
/// first `#` or the opening `"`. Returns the end offset past the
/// matching closer (`"` followed by N `#`s), or None if the input
/// doesn't actually parse as a string literal here so the caller can
/// fall back to identifier handling.
fn try_prefixed_string(bytes: &[u8], start: usize, is_raw: bool) -> Option<usize> {
    let mut hash_count = 0;
    let mut q = start;
    while q < bytes.len() && bytes[q] == b'#' {
        hash_count += 1;
        q += 1;
    }
    // Hashes only legal in raw strings.
    if hash_count > 0 && !is_raw {
        return None;
    }
    if q >= bytes.len() || bytes[q] != b'"' {
        return None;
    }
    if hash_count == 0 {
        return Some(scan_string_body(bytes, q, is_raw));
    }
    // r#"…"# — scan to a closing `"` followed by N `#`s.
    let mut j = q + 1;
    while j < bytes.len() {
        if bytes[j] == b'"' {
            let mut all = true;
            for k in 0..hash_count {
                if j + 1 + k >= bytes.len() || bytes[j + 1 + k] != b'#' {
                    all = false;
                    break;
                }
            }
            if all {
                return Some(j + 1 + hash_count);
            }
        }
        j += 1;
    }
    Some(j) // unterminated — emit to EOL
}

/// Given a slice starting with `<tspan`, find the byte offset *past*
/// the matching `</tspan>`. Returns None on malformed input.
fn find_close_tspan(s: &str) -> Option<usize> {
    // Find the end of the open tag.
    let open_end = s.find('>')?;
    // Then the close tag.
    let close = s[open_end + 1..].find("</tspan>")?;
    Some(open_end + 1 + close + "</tspan>".len())
}

fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

fn is_ident_continue(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hl(line: &str) -> String {
        highlight(line, &mut false)
    }

    #[test]
    fn keywords_get_classed_spans() {
        let h = hl("let x = 5;");
        assert!(h.contains("<tspan class=\"kw\">let</tspan>"), "got: {}", h);
    }

    #[test]
    fn non_keyword_identifier_is_left_alone() {
        let h = hl("foo(bar);");
        assert!(!h.contains("class=\"kw\""), "got: {}", h);
        assert!(h.contains("foo(bar);"));
    }

    #[test]
    fn line_comment_to_eol() {
        let h = hl("let x = 5; // a note");
        assert!(h.contains("<tspan class=\"comment\">// a note</tspan>"));
        // The `let` before the comment should still be a kw.
        assert!(h.contains("<tspan class=\"kw\">let</tspan>"));
    }

    #[test]
    fn string_literal_classed() {
        let h = hl(r#"let s = "hello";"#);
        assert!(h.contains("<tspan class=\"string\">\"hello\"</tspan>"));
        assert!(h.contains("<tspan class=\"kw\">let</tspan>"));
    }

    #[test]
    fn escaped_quote_inside_string_does_not_terminate() {
        let h = hl(r#"let s = "a\"b";"#);
        assert!(h.contains(r#"<tspan class="string">"a\"b"</tspan>"#), "got: {}", h);
    }

    #[test]
    fn existing_annotation_tspan_passes_through() {
        let line = r#"let <tspan data-hash="3">x</tspan> = 5;"#;
        let h = hl(line);
        assert!(h.contains(r#"<tspan data-hash="3">x</tspan>"#));
        assert!(h.contains("<tspan class=\"kw\">let</tspan>"));
    }

    #[test]
    fn keyword_substring_inside_identifier_is_not_classed() {
        let h = hl("let_var = 5;");
        assert!(!h.contains("class=\"kw\""), "got: {}", h);
    }

    #[test]
    fn block_comment_single_line() {
        let h = hl("let /* note */ x = 5;");
        assert!(h.contains(r#"<tspan class="comment">/* note */</tspan>"#));
        assert!(h.contains("<tspan class=\"kw\">let</tspan>"));
    }

    #[test]
    fn html_entities_in_unannotated_text_pass_through() {
        let h = hl("let r = &amp;x;");
        assert!(h.contains("<tspan class=\"kw\">let</tspan>"));
        assert!(h.contains("&amp;x;"));
        assert!(!h.contains("class=\"kw\">amp"));
    }

    // --- Multi-line block comments ---

    #[test]
    fn block_comment_open_sets_state_and_styles_rest_of_line() {
        let mut state = false;
        let h = highlight("let x = 5; /* start", &mut state);
        assert!(h.contains(r#"<tspan class="comment">/* start</tspan>"#), "got: {}", h);
        assert!(h.contains("<tspan class=\"kw\">let</tspan>"));
        assert!(state, "in_block_comment should be set after unclosed /*");
    }

    #[test]
    fn block_comment_continuation_line_is_all_comment() {
        let mut state = true;
        let h = highlight("    middle line", &mut state);
        assert_eq!(h, r#"<tspan class="comment">    middle line</tspan>"#);
        assert!(state, "still inside the block comment");
    }

    #[test]
    fn block_comment_close_clears_state_and_resumes_highlighting() {
        let mut state = true;
        let h = highlight("end */ let y = 6;", &mut state);
        assert!(h.contains(r#"<tspan class="comment">end */</tspan>"#), "got: {}", h);
        assert!(h.contains("<tspan class=\"kw\">let</tspan>"));
        assert!(!state, "in_block_comment should clear after */");
    }

    // --- Prefixed string literals ---

    #[test]
    fn raw_string_prefix_classed_with_prefix() {
        let h = hl(r#"let s = r"hello";"#);
        assert!(h.contains(r#"<tspan class="string">r"hello"</tspan>"#), "got: {}", h);
    }

    #[test]
    fn byte_string_prefix_classed_with_prefix() {
        let h = hl(r#"let s = b"hi";"#);
        assert!(h.contains(r#"<tspan class="string">b"hi"</tspan>"#), "got: {}", h);
    }

    #[test]
    fn byte_raw_string_prefix_classed() {
        let h = hl(r#"let s = br"x";"#);
        assert!(h.contains(r#"<tspan class="string">br"x"</tspan>"#), "got: {}", h);
    }

    #[test]
    fn c_string_prefix_classed() {
        let h = hl(r#"let s = c"x";"#);
        assert!(h.contains(r#"<tspan class="string">c"x"</tspan>"#), "got: {}", h);
    }

    #[test]
    fn raw_string_with_hashes_classed() {
        // r#"a"b"# — embedded `"` doesn't terminate.
        let h = hl(r##"let s = r#"a"b"#;"##);
        assert!(h.contains(r##"<tspan class="string">r#"a"b"#</tspan>"##), "got: {}", h);
    }

    #[test]
    fn raw_string_inside_does_not_apply_backslash_escape() {
        // In r"a\"b", the backslash is literal — the string ends at the first `"` after `a\`.
        // So the literal should be `r"a\"`, then `b";` is left as-is (a stray `b"` won't be
        // matched as a prefixed string because it lacks a closing quote on this line — fall
        // back to "longest valid string" semantics).
        let h = hl(r#"let s = r"a\";"#);
        assert!(h.contains(r#"<tspan class="string">r"a\"</tspan>"#), "got: {}", h);
    }

    #[test]
    fn lone_r_identifier_is_not_treated_as_string_prefix() {
        // `r` followed by a non-quote char must stay a plain identifier.
        let h = hl("let r = 5;");
        assert!(!h.contains("class=\"string\""), "got: {}", h);
        assert!(h.contains("<tspan class=\"kw\">let</tspan>"));
    }

    #[test]
    fn identifier_starting_with_r_is_not_a_prefix() {
        // `range` shouldn't be split into `r` + `ange`.
        let h = hl("let range = 0..10;");
        assert!(!h.contains("class=\"string\""));
        assert!(h.contains("range"));
    }
}
