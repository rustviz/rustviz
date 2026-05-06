//! Plugin regression tests against the corpus of `rustviz-plugin/tests/*.rs`.
//!
//! Each .rs file in the corpus is a Rust program the plugin should be able
//! to visualize. We don't pin exact SVG output (it's noisy and changes with
//! benign tweaks); instead we assert on structural properties:
//!
//!   * `code_panel_string()` and `timeline_panel_string()` return well-formed
//!     SVG (start with `<svg`).
//!   * The timeline contains at least one `tooltip-trigger` element (so the
//!     plugin produced *some* annotations rather than an empty timeline).
//!
//! The curated `EXPECTED_OK` list is the regression floor: anything that
//! passes today should keep passing. New cases get added as we confirm them.
//! Cases known to exercise unsupported features (for-loops, conditional
//! borrows, etc. — see README "Limitations") deliberately stay off this
//! list; failures there are not regressions.
//!
//! These tests use the `RV_RUNNER=local` backend, which requires
//! `cargo install --path rustviz-plugin --locked` to have been run first.
//! `setup.sh` does this for you. CI runs `setup.sh` before `cargo test`.

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use rustviz_lib::Rustviz;

/// Curated subset that should always succeed end-to-end. Each entry is the
/// stem of a file in `../rustviz-plugin/tests/`. Keep this list small and
/// representative; it's the regression floor.
const EXPECTED_OK: &[&str] = &[
    "testMove",
    "testNum",
    "basic_ref",
    "basic_mutref",
    "copy2Caller",
    "ownershipFunctions",
    "testStaticB",
    "testMutB",
    // Field-projection / nested-struct cases — see expected-arrows
    // table below for the per-snippet shape we're locking in.
    "nested_struct_borrow",
    "nested_struct_move",
    "nested_struct_passref",
    "field_method_call",
    "field_through_ref",
];

/// Specific arrow tooltips each snippet must produce. The check is
/// containment (extra arrows are fine), so adding new tracked events
/// to the plugin doesn't break these tests — only removing the
/// listed ones does. Each tuple is (snippet stem, list of expected
/// arrow tooltips, in-source order doesn't matter).
///
/// "Arrow tooltip" = the human-readable string the renderer puts on
/// the `data-tooltip-text` attribute of an arrow `<g>`. We compare
/// the un-HTML-escaped form so multi-segment names like `r.a.b` and
/// punctuation render naturally.
///
/// These tests are the regression bar for issues #71 (`r.s.method()`),
/// #72 (nested-struct reads / borrows / moves), and #73 (`(&r).s`).
const EXPECTED_ARROWS: &[(&str, &[&str])] = &[
    // #72
    ("nested_struct_borrow", &[
        "Move from String::from to r.a.b",
        "Immutable borrow from r.a.b to p",
        "Return immutably borrowed resource from p to r.a.b",
    ]),
    ("nested_struct_move", &[
        "Move from String::from to r.a.b",
        "Move from r.a.b to x",
    ]),
    ("nested_struct_passref", &[
        "Move from String::from to r.a.b",
        "read_a reads from r.a",
    ]),
    // #71
    ("field_method_call", &[
        "Move from String::from to r.s",
        "push_str reads from/writes to r.s",
    ]),
    // #73
    ("field_through_ref", &[
        "Move from String::from to r.s",
        "Immutable borrow from r.s to p",
        "Return immutably borrowed resource from p to r.s",
    ]),
];

fn corpus_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("rustviz-plugin")
        .join("tests")
}

fn read_corpus(name: &str) -> String {
    let path = corpus_dir().join(format!("{}.rs", name));
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {:?}: {}", path, e))
}

fn ensure_plugin_installed() {
    let ok = Command::new("cargo")
        .args(["rv-plugin", "--help"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    assert!(
        ok,
        "`cargo rv-plugin` not on PATH. Run `cargo install --path rustviz-plugin --locked` \
         (or `./setup.sh`) before `cargo test`."
    );
}

fn run_with_local_backend(name: &str) -> Rustviz {
    ensure_plugin_installed();
    // Tests share env between threads; setting this once per test is fine
    // because we only ever set it to the same value. The library reads
    // RV_RUNNER on each Rustviz::new call.
    env::set_var("RV_RUNNER", "local");
    let src = read_corpus(name);
    Rustviz::new(&src).unwrap_or_else(|e| panic!("{} failed: {}", name, e))
}

fn assert_well_formed(name: &str, rv: &Rustviz) {
    let code = rv.code_panel_string();
    let timeline = rv.timeline_panel_string();
    assert!(
        code.trim_start().starts_with("<svg"),
        "{}: code panel is not SVG (starts with: {:?})",
        name,
        code.chars().take(40).collect::<String>()
    );
    assert!(
        timeline.trim_start().starts_with("<svg"),
        "{}: timeline panel is not SVG (starts with: {:?})",
        name,
        timeline.chars().take(40).collect::<String>()
    );
    assert!(
        timeline.contains("tooltip-trigger"),
        "{}: timeline has no tooltip-trigger elements (annotations missing)",
        name
    );
}

/// Extract every arrow tooltip from a rendered timeline panel.
///
/// The renderer wraps each arrow in `<g class="tooltip-trigger"
/// data-tooltip-text="...">` inside `<g id="arrows">`. Tooltip text
/// is HTML-escaped (`&lt;` / `&gt;` / `&quot;`) and contains nested
/// `<span>` markup that we strip before comparing — what we want is
/// the human-visible string ("Immutable borrow from r.a.b to p").
fn arrow_tooltips(timeline_svg: &str) -> Vec<String> {
    let arrows_start = match timeline_svg.find("<g id=\"arrows\">") {
        Some(i) => i,
        None => return Vec::new(),
    };
    let body = &timeline_svg[arrows_start..];
    let mut out = Vec::new();
    for chunk in body.split("data-tooltip-text=\"").skip(1) {
        let raw = match chunk.find('"') {
            Some(end) => &chunk[..end],
            None => continue,
        };
        // Un-HTML-escape the small set the renderer emits.
        let unescaped = raw
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&amp;", "&");
        // Strip nested span markup; what's left is the visible text.
        let mut clean = String::with_capacity(unescaped.len());
        let mut in_tag = false;
        for ch in unescaped.chars() {
            if ch == '<' { in_tag = true; continue; }
            if ch == '>' { in_tag = false; continue; }
            if !in_tag { clean.push(ch); }
        }
        out.push(clean);
    }
    out
}

#[test]
fn corpus_expected_arrows_present() {
    let mut failures = Vec::new();
    for (name, expected) in EXPECTED_ARROWS {
        let res = std::panic::catch_unwind(|| {
            let rv = run_with_local_backend(name);
            let actual = arrow_tooltips(&rv.timeline_panel_string());
            let mut missing: Vec<&str> = Vec::new();
            for want in *expected {
                if !actual.iter().any(|a| a == want) {
                    missing.push(*want);
                }
            }
            if !missing.is_empty() {
                panic!(
                    "missing arrows {:?}; actual arrows were:\n  {}",
                    missing,
                    actual.join("\n  ")
                );
            }
        });
        if let Err(payload) = res {
            let msg = payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "<non-string panic>".to_string());
            failures.push(format!("{}: {}", name, msg));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} arrow assertions failed:\n  {}",
        failures.len(),
        EXPECTED_ARROWS.len(),
        failures.join("\n  ")
    );
}

#[test]
fn corpus_expected_ok_produce_well_formed_svg() {
    let mut failures = Vec::new();
    for name in EXPECTED_OK {
        // Don't unwind across cases — collect all failures so a single broken
        // case doesn't mask others.
        match std::panic::catch_unwind(|| {
            let rv = run_with_local_backend(name);
            assert_well_formed(name, &rv);
        }) {
            Ok(()) => {}
            Err(payload) => {
                let msg = payload
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_else(|| "<non-string panic>".to_string());
                failures.push(format!("{}: {}", name, msg));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} corpus cases regressed:\n  {}",
        failures.len(),
        EXPECTED_OK.len(),
        failures.join("\n  ")
    );
}
