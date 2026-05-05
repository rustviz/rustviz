//! Structural assertions on borrow visualization for canonical borrow
//! shapes. Pins the gains from the borrow-visualization PR so future
//! changes don't silently regress them.
//!
//! Each test compiles a snippet through the local plugin backend and
//! inspects the resulting timeline panel SVG for properties that
//! describe correctness without pinning exact pixel coordinates:
//!
//!   * The expected hover strings are present.
//!   * No phantom `from s to *s` (the pre-fix bug).
//!   * The borrow-region path (`<path ... staticref ...>` or its
//!     mutref counterpart) has a non-zero vertical extent.
//!   * The lender's timeline has at least one Solid segment AND at
//!     least one Hollow segment (state transitions visible).
//!
//! These tests use the same `RV_RUNNER=local` backend as `corpus.rs`
//! and require `cargo install --path rustviz2-plugin --locked` first.

use std::env;

use rustviz2::Rustviz;

fn run(src: &str) -> Rustviz {
    env::set_var("RV_RUNNER", "local");
    Rustviz::new(src).unwrap_or_else(|e| panic!("plugin error:\n{}", e))
}

/// Strip the styled `&lt;span …&gt;…&lt;/span&gt;` wrappers that
/// `fmt_style` puts around variable names in tooltip text. Tests
/// don't care about the inline-styling noise; they want to assert on
/// the prose. After this pass `Immutable borrow from x to r` matches
/// even though the raw SVG has spans wrapping `x` and `r`.
fn strip_tooltip_styling(timeline: &str) -> String {
    let mut out = timeline.to_string();
    out = out.replace("&lt;/span&gt;", "");
    // Drop everything from `&lt;span` up to the closing `&gt;`. The
    // intermediate text (the inline `style="…"`) varies, but the
    // closing `&gt;` is the first one *after* the opening tag.
    while let Some(start) = out.find("&lt;span") {
        if let Some(rel) = out[start..].find("&gt;") {
            out.replace_range(start..start + rel + 4, "");
        } else {
            break;
        }
    }
    out
}

fn timeline_of(src: &str) -> String {
    strip_tooltip_styling(&run(src).timeline_panel_string())
}

/// Find each `d="…"` on a `staticref`/`mutref` borrow-region path and
/// return its raw value. Useful for verifying the trapezoid spans a
/// real range rather than collapsing to a point.
fn ref_line_paths(timeline: &str) -> Vec<String> {
    let mut out = Vec::new();
    // Crude SVG scrape — fine for tests, where the format is stable.
    for tag in timeline.split("<path ") {
        if !(tag.contains("staticref") || tag.contains("mutref")) {
            continue;
        }
        if let Some(start) = tag.find("d=\"") {
            let rest = &tag[start + 3..];
            if let Some(end) = rest.find('"') {
                out.push(rest[..end].to_string());
            }
        }
    }
    out
}

/// `d="M x1 y1 l dx dy v V l -dx dy"` — extract the `v V` magnitude.
/// Returns 0 if the path doesn't match the expected shape.
fn ref_line_v(d: &str) -> f64 {
    let v_marker = match d.find(" v ") {
        Some(i) => i + 3,
        None => return 0.0,
    };
    let rest = &d[v_marker..];
    let end = rest.find(' ').unwrap_or(rest.len());
    rest[..end].parse::<f64>().unwrap_or(0.0)
}

/// True if at least one timeline `<line class="solid" data-hash="HASH">`
/// segment exists.
fn has_solid_segment_for(timeline: &str, hash: u64) -> bool {
    let needle = format!("data-hash=\"{}\"", hash);
    timeline
        .split("<line ")
        .any(|seg| seg.contains(&needle) && seg.contains("class=\"solid"))
}

/// True if at least one hollow `<path ... data-hash=HASH ... class="hollow…">`
/// segment exists.
fn has_hollow_segment_for(timeline: &str, hash: u64) -> bool {
    let needle = format!("data-hash=\"{}\"", hash);
    timeline
        .split("<path ")
        .any(|seg| seg.contains(&needle) && seg.contains("class=\"hollow"))
}

/// Hash of the n-th declared variable; rustviz2 assigns hashes sequentially
/// in declaration order. The function RAP for `String::from` lands at hash 1
/// in every snippet that uses it, so the first user-declared variable is
/// usually hash 2. Adjust per-test if needed.
const FIRST_VAR_HASH: u64 = 2;

/// Look up the timeline hash assigned to a label (e.g. "r" or "r.w") so
/// tests don't have to hard-code declaration order. Returns `None` if no
/// matching `<text … class="label" … data-hash=N>label</text>` is found.
fn label_hash(timeline: &str, label: &str) -> Option<u64> {
    for tag in timeline.split("<text ") {
        if !tag.contains("class=\"label") {
            continue;
        }
        // The label text follows the opening tag's `>`.
        let after_open = match tag.find('>') {
            Some(i) => &tag[i + 1..],
            None => continue,
        };
        let text_end = after_open.find('<').unwrap_or(after_open.len());
        if after_open[..text_end].trim() == label {
            let hash_marker = match tag.find("data-hash=\"") {
                Some(i) => i + "data-hash=\"".len(),
                None => continue,
            };
            let rest = &tag[hash_marker..];
            let end = rest.find('"').unwrap_or(rest.len());
            if let Ok(h) = rest[..end].parse::<u64>() {
                return Some(h);
            }
        }
    }
    None
}

#[test]
fn fn_param_ref_loan_spans_body_and_no_phantom_die() {
    // Canonical screenshot example: f(&x) with fn f(s: &String) { *s }.
    // Pre-fix: emitted "Return immutably borrowed resource from s to *s"
    // at the signature line, with a v=0 ref-line trapezoid.
    let src = "\
fn main() {
    let x = String::from(\"hello\");
    f(&x);
    println!(\"{}\", x);
}

fn f(s: &String) {
    println!(\"{}\", *s);
}
";
    let timeline = timeline_of(src);

    assert!(
        !timeline.contains("from s to *s"),
        "phantom `from s to *s` tooltip is back"
    );
    assert!(
        timeline.contains("s is an immutable borrow from the caller"),
        "fn-param-ref init tooltip missing"
    );
    assert!(
        timeline.contains("s holds an immutable reference"),
        "ref-line tooltip missing"
    );

    // The loan region for s should span the fn body (lines 7–9), so
    // the staticref path's v should be substantially > 0.
    let lines = ref_line_paths(&timeline);
    assert!(!lines.is_empty(), "no staticref/mutref path emitted for s");
    let v = ref_line_v(&lines[0]);
    assert!(
        v > 10.0,
        "fn-param-ref loan region collapsed (v={}); path={:?}",
        v,
        lines[0]
    );

    // x is `let x` (immutable), so its timeline renders Hollow
    // throughout — the loan is visualized on s's side via the
    // dashed trapezoid, not by varying the lender's stroke style.
    assert!(
        has_hollow_segment_for(&timeline, FIRST_VAR_HASH),
        "x's hollow timeline segment missing"
    );
}

#[test]
fn within_scope_immutable_borrow_renders_full_loan() {
    // `let r = &x; use r; use x` — borrow spans declaration to last
    // use of r, then x recovers FullPrivilege.
    let src = "\
fn main() {
    let x = String::from(\"hello\");
    let r = &x;
    println!(\"{}\", r);
    println!(\"{}\", x);
}
";
    let timeline = timeline_of(src);

    assert!(timeline.contains("r holds an immutable reference"));
    assert!(timeline.contains("Immutable borrow from x to r"));
    assert!(timeline.contains("Return immutably borrowed resource from r to x"));

    let lines = ref_line_paths(&timeline);
    assert!(!lines.is_empty(), "no staticref path emitted for r");
    assert!(
        ref_line_v(&lines[0]) > 10.0,
        "within-scope loan region collapsed: {:?}",
        lines[0]
    );

    assert!(has_hollow_segment_for(&timeline, FIRST_VAR_HASH));
}

#[test]
fn multiple_immutable_borrows_share_the_lender() {
    // `let y = &x; let z = &x;` — both refs alive simultaneously
    // through their last use; x is Hollow across the union.
    let src = "\
fn main() {
    let x = String::from(\"hello\");
    let y = &x;
    let z = &x;
    println!(\"{} {}\", y, z);
}
";
    let timeline = timeline_of(src);

    assert!(timeline.contains("Immutable borrow from x to y"));
    assert!(timeline.contains("Immutable borrow from x to z"));

    let lines = ref_line_paths(&timeline);
    assert!(
        lines.len() >= 2,
        "expected two ref-line trapezoids (one each for y and z), got {}",
        lines.len()
    );
    for d in &lines {
        assert!(
            ref_line_v(d) > 10.0,
            "loan trapezoid collapsed: {:?}",
            d
        );
    }
}

#[test]
fn immutable_struct_renders_hollow_and_drops_at_oos() {
    // `let r = Rect { .. }` — r is an immutable Struct binding.
    // Both r and its fields r.w / r.h should render Hollow (not
    // Solid; they can't be reassigned). Drop-indicator wording
    // diverges by Copy-ness: Rect is non-Copy (no derive), so r
    // shows "Its resource is dropped" at OOS; r.w / r.h are u32
    // which IS Copy, so they get the plain "goes out of scope"
    // tooltip — Copy types have no destructor and the renderer
    // shouldn't pretend otherwise.
    let src = "\
struct Rect { w: u32, h: u32 }

fn main() {
    let r = Rect { w: 30, h: 50 };
    println!(\"{} {}\", r.w, r.h);
}
";
    let timeline = timeline_of(src);

    // Non-Copy struct: drop tooltip + drop dot.
    let r_drop = "r goes out of scope. Its resource is dropped.";
    assert!(
        timeline.contains(r_drop),
        "expected drop tooltip for r (non-Copy struct), not found"
    );

    // Copy fields: plain OOS tooltip, NO "Its resource is dropped".
    for who in ["r.w", "r.h"] {
        let plain = format!("{} goes out of scope", who);
        assert!(
            timeline.contains(&plain),
            "expected plain OOS tooltip for {}, not found",
            who
        );
        let bad = format!("{} goes out of scope. Its resource is dropped.", who);
        assert!(
            !timeline.contains(&bad),
            "{} is Copy (u32) — should NOT carry the drop suffix",
            who
        );
    }

    // r is immutable → no Solid segment on r's timeline; Hollow
    // throughout. Hash assignment for the struct case is
    // implementation-detail-y (struct itself + each field each take
    // a hash), so look r up by its label rather than hard-coding.
    let r_hash = label_hash(&timeline, "r")
        .expect("could not locate hash for label 'r' in timeline");
    assert!(
        has_hollow_segment_for(&timeline, r_hash),
        "r's hollow segment missing — immutable struct should be Hollow"
    );
    assert!(
        !has_solid_segment_for(&timeline, r_hash),
        "r is immutable; should not render any Solid segment"
    );
}

#[test]
fn copy_owner_oos_omits_drop_indicator_and_suffix() {
    // Primitives implement Copy → no Drop glue runs at OOS, so the
    // tooltip should NOT carry "Its resource is dropped" and no
    // drop-triangle dot should be emitted. A non-Copy owner in the
    // same snippet still gets the drop suffix; lets us verify that
    // the gate is per-RAP, not workspace-wide.
    let src = "\
fn main() {
    let n: i32 = 5;
    let s = String::from(\"hi\");
    println!(\"{} {}\", n, s);
}
";
    let timeline = timeline_of(src);

    // i32 owner — bare OOS, no drop suffix.
    assert!(
        timeline.contains("n goes out of scope"),
        "expected plain OOS tooltip for n"
    );
    assert!(
        !timeline.contains("n goes out of scope. Its resource is dropped."),
        "n is i32 (Copy) — should NOT carry the drop suffix"
    );

    // String owner — drop suffix present.
    assert!(
        timeline.contains("s goes out of scope. Its resource is dropped."),
        "expected drop tooltip for s (non-Copy String)"
    );

    // No drop-dot group should be emitted for n. The drop-dot
    // template wraps `<circle>` + `<polygon>` in a single
    // `<g data-hash="{hash}" ... data-tooltip-text="{...}">` —
    // looking for an n-hashed group whose tooltip mentions the
    // drop suffix detects the drop wrapper specifically (regular
    // OOS dots don't carry that suffix).
    let n_hash = label_hash(&timeline, "n")
        .expect("could not locate hash for label 'n' in timeline");
    let drop_group = format!("data-hash=\"{}\"", n_hash);
    for line in timeline.lines() {
        if line.contains("<g ")
            && line.contains(&drop_group)
            && line.contains("Its resource is dropped")
        {
            panic!("found a drop wrapper bound to n (Copy type): {}", line);
        }
    }
}

#[test]
fn struct_with_ref_field_models_borrow_chain() {
    // Tutorial "Struct with lifetime" example, condensed: a value
    // sliced out of `n` flows through a method chain into `first`,
    // which is then stored in a struct field `i.p`. After the
    // borrow-chain + ref-field plumbing, both `first` and `i.p`
    // should appear as immutable borrows of `n`, each with a
    // visible loan-region trapezoid, and `i.p` should land inside
    // i's struct bounding box.
    let src = "\
struct Excerpt<'a> {
    p: &'a str,
}

fn some_function() {
    let n = String::from(\"Ok. I'm fine.\");
    let first = n.split('.').next().expect(\"...\");
    let i = Excerpt { p: first };
}

fn main() {
    some_function();
}
";
    let timeline = timeline_of(src);

    // first acquires its borrow from n via the method chain (the
    // bug being closed: pre-fix this rendered as Copy from
    // expect()).
    assert!(
        timeline.contains("Immutable borrow from n to first"),
        "first didn't get a Borrow event from n; got tooltips:\n{}",
        timeline
    );
    assert!(
        timeline.contains("first holds an immutable reference"),
        "first's ref-line tooltip missing"
    );

    // i.p inherits the same borrow (Copy of a ref → ref RAP).
    assert!(
        timeline.contains("i.p holds an immutable reference"),
        "i.p's ref-line tooltip missing — likely still modelled as a Struct field"
    );

    // print_lifetimes resolves i.p's transitive lender (n) and
    // emits the matching return-of-borrow at end of scope.
    assert!(
        timeline.contains("Return immutably borrowed resource from i.p to n"),
        "i.p didn't return its borrow to n"
    );

    // Both ref-lines render with a real trapezoid range.
    let lines = ref_line_paths(&timeline);
    assert!(
        lines.len() >= 2,
        "expected ref-lines for both first and i.p, got {}",
        lines.len()
    );
    for d in &lines {
        assert!(
            ref_line_v(d) > 10.0,
            "loan trapezoid collapsed: {:?}",
            d
        );
    }

    // Struct bounding box around i + i.p is drawn.
    let raw = run(src).timeline_panel_string();
    assert!(
        raw.contains("<rect id=\""),
        "struct bounding box missing for Excerpt"
    );
}

#[test]
fn struct_box_renders_when_struct_group_is_last_in_iteration() {
    // The bounding `<rect>` around r/r.field timelines is finalised
    // when compute_column_layout transitions from a struct member
    // to a non-struct RAP. If the struct happens to occupy the
    // highest hashes (e.g. when fn parameters of ref types take
    // smaller hashes — `&self` and `rect: &Rectangle` here), no
    // non-struct RAP follows and the box used to never be pushed.
    let src = "\
struct Rectangle {
    width: u32,
    height: u32,
}

impl Rectangle {
    fn area(&self) -> u32 { self.width * self.height }
}

fn print_area(rect: &Rectangle) {
    println!(\"{}\", rect.area());
}

fn main() {
    let r = Rectangle { width: 30, height: 50 };
    print_area(&r);
}
";
    let timeline = run(src).timeline_panel_string();

    // Box template emits `<rect id="HASH" …/>`. We don't pin
    // dimensions; existence is the property under test.
    assert!(
        timeline.contains("<rect id=\""),
        "expected struct bounding box <rect> in timeline panel, found none"
    );
}

#[test]
fn mutable_borrow_takes_lender_to_revoked() {
    // `let y = &mut x` — lender x transitions to RevokedPrivilege
    // during the loan; y is the active mutable reference.
    let src = "\
fn main() {
    let mut x = String::from(\"hello\");
    let y = &mut x;
    y.push_str(\" world\");
    println!(\"{}\", x);
}
";
    let timeline = timeline_of(src);

    assert!(timeline.contains("Mutable borrow from x to y"));
    assert!(timeline.contains("Return mutably borrowed resource from y to x"));
    assert!(timeline.contains("y holds a mutable reference"));

    let lines = ref_line_paths(&timeline);
    assert!(!lines.is_empty());
    assert!(
        ref_line_v(&lines[0]) > 10.0,
        "mutable loan region collapsed: {:?}",
        lines[0]
    );
}

#[test]
fn struct_field_definitions_render_with_their_types() {
    // Regression: annotate_struct_field used to overwrite the
    // entire `name: Type` declaration with just the field name,
    // erasing the type annotation. This pins that the rendered
    // code panel keeps `x: i32` and `y: String` intact.
    let src = "\
struct Foo {
    x: i32,
    y: String,
}

fn main() {
    let _y = String::from(\"bar\");
    let f = Foo { x: 5, y: _y };
    println!(\"{}\", f.x);
    println!(\"{}\", f.y);
}
";
    let code_panel = run(src).code_panel_string();
    let stripped: String = code_panel
        .lines()
        .map(|l| {
            // strip inline tags so we can match the bare prose
            let mut out = l.to_string();
            while let (Some(o), Some(c)) = (out.find('<'), out.find('>')) {
                if o < c { out.replace_range(o..=c, ""); } else { break; }
            }
            out
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        stripped.contains("x: i32,"),
        "field type `i32` missing from rendered struct definition"
    );
    assert!(
        stripped.contains("y: String,"),
        "field type `String` missing from rendered struct definition"
    );
}

#[test]
fn struct_box_aligns_with_per_fn_label_row() {
    // The struct bounding `<rect>` used to hardcode y=50, which
    // matched the legacy "all labels at top of SVG" layout. After
    // per-fn label positioning, the box has to follow its parent
    // struct's fn — otherwise it floats away from the columns it's
    // supposed to enclose. Pin: the box's y_top equals the f-group
    // label_y minus 20 (the box vertically centers on the label
    // row, which is 30px tall).
    let src = "\
struct Foo {
    x: i32,
    y: String,
}

fn main() {
    let f = Foo { x: 5, y: String::from(\"bar\") };
    println!(\"{} {}\", f.x, f.y);
}
";
    let timeline = run(src).timeline_panel_string();

    // Tiny attribute scrape: find an attr `key="value"` inside a tag
    // substring. Saves pulling regex into the dev-deps just for these.
    fn attr<'s>(tag: &'s str, key: &str) -> Option<&'s str> {
        let needle = format!("{}=\"", key);
        let start = tag.find(&needle)? + needle.len();
        let end = tag[start..].find('"')? + start;
        Some(&tag[start..end])
    }

    // Locate the f-group variable label y. Labels render as
    // `<text x="..." y="Y" data-hash="H" class="label …" …>f<tspan>|</tspan>*f</text>`
    // for refs and `…>name</text>` for owners. We want the parent
    // struct label `f`, which is an owner-style Struct RAP.
    let label_y: i64 = timeline
        .lines()
        .find_map(|l| {
            if !l.contains("class=\"label") { return None; }
            // strip nested tags from the body so `f<tspan>|</tspan>*f`
            // collapses to `f|*f`; bare label `f` stays `f`.
            let open = l.find("<text")?;
            let body_start = l[open..].find('>')? + open + 1;
            let body_end = l[body_start..].find("</text>")? + body_start;
            let mut bare = String::new();
            let mut depth = 0;
            for ch in l[body_start..body_end].chars() {
                match ch { '<' => depth += 1, '>' => depth -= 1, _ if depth == 0 => bare.push(ch), _ => {} }
            }
            if bare.trim() == "f" || bare.starts_with("f|") {
                attr(&l[open..body_start], "y")?.parse().ok()
            } else {
                None
            }
        })
        .expect("could not locate label `f` in timeline");

    // Locate the struct box `<rect …/>`.
    let rect_line = timeline
        .lines()
        .find(|l| l.trim_start().starts_with("<rect"))
        .expect("expected struct bounding box <rect> in timeline");
    let box_y: i64 = attr(rect_line, "y").and_then(|s| s.parse().ok()).unwrap();
    let box_h: i64 = attr(rect_line, "height").and_then(|s| s.parse().ok()).unwrap();

    // Box vertically centers on label row: top = label_y - 20.
    assert_eq!(
        box_y,
        label_y - 20,
        "struct box y={} does not align with f-group label_y={} (expected box_y = label_y - 20)",
        box_y, label_y
    );
    assert_eq!(box_h, 30, "struct box height changed unexpectedly");
}
