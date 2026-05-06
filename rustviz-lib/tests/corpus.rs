//! Plugin regression tests against the corpus of `rustviz-plugin/tests/*.rs`.
//!
//! Each .rs file in the corpus is a Rust program the plugin should be able
//! to visualize. We don't pin exact SVG output (it's noisy and changes with
//! benign tweaks); instead we assert on three layers:
//!
//!   * `corpus_expected_ok_produce_well_formed_svg` — the regression floor.
//!     Every `EXPECTED_OK` snippet returns well-formed SVG (`<svg ...`) for
//!     both panels and the timeline carries at least one `tooltip-trigger`
//!     element (i.e. the plugin produced *some* annotations).
//!
//!   * `corpus_expected_tooltips_present` — the behaviour bar. For every
//!     entry in `EXPECTED_TOOLTIPS`, the listed `must_contain` strings must
//!     appear verbatim as `data-tooltip-text` values in the rendered
//!     timeline, and the `must_not_contain` strings must *not* appear.
//!     Containment semantics on both sides — extra tooltips are fine, only
//!     the listed ones are load-bearing.
//!
//!   * For features known to be unsupported (for-loops, conditional borrows,
//!     smart pointers, etc. — see README "Limitations") snippets stay off
//!     these lists; failures there aren't regressions until the feature is
//!     implemented.
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
///
/// Snippets are grouped by feature area in source order so a glance at
/// the list shows which categories are covered.
const EXPECTED_OK: &[&str] = &[
    // — Existing canon: ownership / move / copy / basic refs.
    "testMove",
    "testNum",
    "basic_ref",
    "basic_mutref",
    "copy2Caller",
    "ownershipFunctions",
    "testStaticB",
    "testMutB",
    // — Field-projection cases (added with #84).
    "nested_struct_borrow",
    "nested_struct_move",
    "nested_struct_passref",
    "field_method_call",
    "field_through_ref",
    // — References: more nuanced shapes.
    "reborrow",
    "nll_two_borrows",
    // — Reassignment / move-out / take-and-return.
    "reassign_with_drop",
    "move_return",
    "take_and_return",
    // — Stdlib method calls.
    "string_push_str",
    "string_len",
    // — Stdlib indexing / slicing (#75).
    "vec_slice",
    // — User-defined inherent methods.
    "inherent_method_rectangle",
    // — Lifetime annotations.
    "lifetime_excerpt",
    // — Generics.
    "generic_fn",
    // — Shadowing.
    "shadowing_basic",
    // — Marker comments (skip on let, hide on fn).
    "skip_marker_let",
    "hide_marker_fn",
    // — Conditionals as expression RHS (the supported subset).
    "if_as_let_rhs",
    // — Smart-pointer wrappers (#76): rendered as opaque single-owner
    //   columns rather than recursing into Unique/NonNull/PhantomData
    //   internals.
    "box_string",
    "rc_clone",
    "box_dyn",
    // — Closure captures (#79): capture arrows render for both
    //   `move` and borrow closures.
    "closure_move_single",
    "closure_move_multi",
    "closure_borrow_imm",
    "closure_borrow_mut",
];

/// Tooltip-level expectations per snippet. `must_contain` strings have to
/// appear verbatim as a `data-tooltip-text` value somewhere in the rendered
/// timeline; `must_not_contain` strings must not. Both checks are
/// containment, so adding new tooltips downstream doesn't break the test —
/// only removing a listed one (or producing one that's been forbidden) does.
struct TooltipExpect {
    name: &'static str,
    must_contain: &'static [&'static str],
    must_not_contain: &'static [&'static str],
}

const EXPECTED_TOOLTIPS: &[TooltipExpect] = &[
    // ─── Ownership / moves / copies ──────────────────────────────────
    TooltipExpect {
        name: "testMove",
        must_contain: &[
            "Move from String::from to s",
            "Move from s to takes_and_drops",
        ],
        must_not_contain: &[],
    },
    TooltipExpect {
        name: "ownershipFunctions",
        must_contain: &[
            "Move from String::from to s",
            "Move from s to takes_ownership",
            "Copy from x to makes_copy",
        ],
        must_not_contain: &[],
    },
    TooltipExpect {
        name: "copy2Caller",
        must_contain: &[
            "Copy from f to x",
            "y's resource is moved to the caller",
        ],
        must_not_contain: &[],
    },
    TooltipExpect {
        name: "reassign_with_drop",
        must_contain: &[
            "Move from x to y",
            "y acquires ownership of a new resource; its previous resource is dropped",
        ],
        must_not_contain: &[],
    },

    // ─── References: borrow + return ─────────────────────────────────
    TooltipExpect {
        name: "basic_ref",
        must_contain: &[
            "Immutable borrow from x to y",
            "Return immutably borrowed resource from y to x",
        ],
        must_not_contain: &[],
    },
    TooltipExpect {
        name: "basic_mutref",
        must_contain: &[
            "Mutable borrow from x to y",
            "Return mutably borrowed resource from y to x",
        ],
        must_not_contain: &[],
    },
    TooltipExpect {
        name: "testStaticB",
        must_contain: &[
            "Move from String::from to my_string",
            "Immutable borrow from my_string to my_str",
            "Return immutably borrowed resource from my_str to my_string",
        ],
        must_not_contain: &[],
    },
    TooltipExpect {
        name: "testMutB",
        must_contain: &[
            "Move from String::from to greeting",
            "Mutable borrow from greeting to r1",
            "Return mutably borrowed resource from r1 to greeting",
        ],
        must_not_contain: &[],
    },
    TooltipExpect {
        name: "reborrow",
        must_contain: &[
            "Mutable borrow from s to r",
            "Mutable borrow from r to r2",
            "Return mutably borrowed resource from r2 to *r",
            "Return mutably borrowed resource from r to s",
            "push_str reads from/writes to r2",
        ],
        must_not_contain: &[],
    },
    TooltipExpect {
        name: "nll_two_borrows",
        // Two consecutive &mut borrows, with the loan released between
        // them. The renderer should draw both borrow-and-return pairs
        // and a fn-call interaction at each `world(...)` site.
        must_contain: &[
            "Mutable borrow from x to y",
            "Return mutably borrowed resource from y to x",
            "Mutable borrow from x to z",
            "Return mutably borrowed resource from z to x",
            "world reads from/writes to y",
            "world reads from/writes to z",
        ],
        must_not_contain: &[],
    },

    // ─── Field-projection cases (regression bar for #71/#72/#73) ─────
    TooltipExpect {
        name: "nested_struct_borrow",
        must_contain: &[
            "Move from String::from to r.a.b",
            "Immutable borrow from r.a.b to p",
            "Return immutably borrowed resource from p to r.a.b",
        ],
        must_not_contain: &[],
    },
    TooltipExpect {
        name: "nested_struct_move",
        must_contain: &[
            "Move from String::from to r.a.b",
            "Move from r.a.b to x",
        ],
        must_not_contain: &[],
    },
    TooltipExpect {
        name: "nested_struct_passref",
        must_contain: &[
            "Move from String::from to r.a.b",
            "read_a reads from r.a",
        ],
        must_not_contain: &[],
    },
    TooltipExpect {
        name: "field_method_call",
        must_contain: &[
            "Move from String::from to r.s",
            "push_str reads from/writes to r.s",
        ],
        must_not_contain: &[],
    },
    TooltipExpect {
        name: "field_through_ref",
        must_contain: &[
            "Move from String::from to r.s",
            "Immutable borrow from r.s to p",
            "Return immutably borrowed resource from p to r.s",
        ],
        must_not_contain: &[],
    },

    // ─── Function calls / returns ────────────────────────────────────
    TooltipExpect {
        name: "move_return",
        must_contain: &[
            "Move from String::from to s",
            "s's resource is moved to the caller",
            "Move from make to r",
        ],
        must_not_contain: &[],
    },
    TooltipExpect {
        name: "take_and_return",
        must_contain: &[
            "Move from s to take_and_return",
            "Move from take_and_return to s",
        ],
        must_not_contain: &[],
    },

    // ─── Stdlib method calls ────────────────────────────────────────
    TooltipExpect {
        name: "string_push_str",
        // `&mut self` method → PassByMutableReference — rendered as
        // "X reads from/writes to Y" on the fn-icon dot.
        must_contain: &[
            "Move from String::from to s",
            "push_str reads from/writes to s",
        ],
        must_not_contain: &[],
    },
    TooltipExpect {
        name: "vec_slice",
        // `let v = vec![..]` desugars through a macro into a Call whose
        // function never gets registered as a RAP. The fix in #75 emits a
        // plain Bind ("v acquires ownership") instead of crashing. `&v[..]`
        // attributes the borrow to `v` (Vec collapses via #76's non-local
        // ADT rule), matching how `&s` works for `s: String`.
        must_contain: &[
            "v acquires ownership of a resource",
            "Immutable borrow from v to p",
            "Return immutably borrowed resource from p to v",
        ],
        must_not_contain: &[
            // Vec internals shouldn't leak into the timeline as separate
            // columns (regression guard for `ty_is_special_owner`).
            "v.buf, immutable",
            "v.len, immutable",
        ],
    },
    TooltipExpect {
        name: "string_len",
        // `&self` method → PassByStaticReference — "X reads from Y" on
        // the fn-icon dot. The Copy return value lands in n.
        must_contain: &[
            "Move from String::from to s",
            "len reads from s",
            "Copy from len to n",
        ],
        must_not_contain: &[],
    },

    // ─── User-defined inherent methods ──────────────────────────────
    TooltipExpect {
        name: "inherent_method_rectangle",
        // The Rectangle/area pattern is the canonical "method on a
        // user struct" shape we know works. We don't test for r.area's
        // call-site arrow today — see #74 for the ref-arg-call-site
        // PassByRef arrow that doesn't render yet.
        must_contain: &[
            "print_area reads from r",
            "r acquires ownership of a resource",
            "r goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[],
    },

    // ─── Lifetimes ──────────────────────────────────────────────────
    TooltipExpect {
        name: "lifetime_excerpt",
        // Excerpt<'a> { p: &'a str } — `e.p` is rendered as a borrower
        // of `s`. Borrow + matching return both appear.
        must_contain: &[
            "Move from String::from to s",
            "Immutable borrow from s to e.p",
            "Return immutably borrowed resource from e.p to s",
        ],
        must_not_contain: &[],
    },

    // ─── Generics ───────────────────────────────────────────────────
    TooltipExpect {
        name: "generic_fn",
        must_contain: &[
            "Move from s to id",
            "Move from id to t",
        ],
        must_not_contain: &[],
    },

    // ─── Shadowing ──────────────────────────────────────────────────
    TooltipExpect {
        name: "shadowing_basic",
        // Both `let s = ...` lines should bind a String to `s`. The
        // first `s` is dropped at the shadow site.
        must_contain: &[
            "Move from String::from to s",
            "s goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[],
    },

    // ─── Marker comments (skip / hide) ──────────────────────────────
    TooltipExpect {
        name: "skip_marker_let",
        // `let q = ...; // rustviz: skip` — every event touching `q`
        // is dropped. The rendered timeline must mention `s` but never
        // `q`.
        must_contain: &[
            "Move from String::from to s",
            "s goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "q acquires ownership of a resource",
            "q goes out of scope. Its resource is dropped.",
            "Move from String::from to q",
        ],
    },
    TooltipExpect {
        name: "hide_marker_fn",
        // `// rustviz: hide` on `helper` — the call-site `Move s ->
        // helper` arrow still fires (Function RAP created at the call
        // site), but `helper`'s body isn't traversed so its
        // `some_string` parameter never appears as a column / state.
        must_contain: &[
            "Move from String::from to s",
            "Move from s to helper",
        ],
        must_not_contain: &[
            "some_string acquires ownership from the caller",
            "some_string goes out of scope. Its resource is dropped.",
        ],
    },

    // ─── Conditionals as expression RHS ─────────────────────────────
    TooltipExpect {
        name: "if_as_let_rhs",
        // `let s = if cond { String::from(..) } else { String::from(..) };`
        // Both branches' moves into `s` show up; `s` ends up the
        // owner regardless of branch.
        must_contain: &[
            "Move from String::from to s",
            "s goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[],
    },

    // ─── Smart-pointer wrappers (#76) ───────────────────────────────
    // Box / Rc / Box<dyn T> render as a single owning column, not as
    // their internal struct internals. The `must_not_contain` list
    // pins the regression: if any of these strings come back, #84's
    // recursive struct-field walker is leaking wrapper internals
    // into the timeline again.
    TooltipExpect {
        name: "box_string",
        must_contain: &[
            "Move from Box::new to b",
        ],
        must_not_contain: &[
            "b.0, immutable",
            "b.0.pointer, immutable",
            "b.0.pointer.pointer, immutable",
            "b.0._marker, immutable",
            "b.1, immutable",
        ],
    },
    TooltipExpect {
        name: "rc_clone",
        // `Rc::clone(&r)` is just a regular function call: `&r` is
        // a static borrow into Rc::clone, the return value moves
        // into r2. Shared-ownership semantics aren't visualized
        // (per the issue's "out of scope for the panic fix" note).
        must_contain: &[
            "Move from Rc::new to r",
            "Move from Rc::clone to r2",
            "Rc::clone reads from r",
        ],
        must_not_contain: &[
            "r.ptr, immutable",
            "r.ptr.pointer, immutable",
            "r.phantom, immutable",
            "r.alloc, immutable",
            "r2.ptr, immutable",
            "r2.alloc, immutable",
        ],
    },
    TooltipExpect {
        name: "box_dyn",
        must_contain: &[
            "Move from Box::new to b",
        ],
        must_not_contain: &[
            "b.0, immutable",
            "b.0.pointer, immutable",
            "b.0._marker, immutable",
            "b.1, immutable",
        ],
    },

    // ─── Closure captures (#79) ──────────────────────────────────────
    //
    // Move/borrow events into a closure binding render with
    // capture-flavoured arrow labels (`Closure capture (move)` /
    // `… (immutable borrow)` / `… (mutable borrow)`) instead of the
    // generic `Move` / `Immutable borrow` / `Mutable borrow`. The
    // closure binding's scope-end also gets a "captured resources
    // are dropped" message in place of the generic "resource is
    // dropped" suffix.
    TooltipExpect {
        name: "closure_move_single",
        must_contain: &[
            "Move from String::from to s",
            "Closure capture (move) from s to f",
            "Closure f captures: s (moved)",
            "f goes out of scope. Its captured resources are dropped.",
            // Move closure → f's timeline says it owns a closure
            // that owns a resource (the moved upvar).
            "f owns a closure which owns a resource",
        ],
        must_not_contain: &[
            // Generic Move-arrow label would obscure the capture.
            "Move from s to f",
            // Per-capture closure-side dots are suppressed in
            // favour of the combined Bind dot.
            "Closure f captures (moves) s's resource",
            // Generic owner state message would obscure the
            // closure-vs-resource distinction.
            "f is the owner of the resource",
        ],
    },
    TooltipExpect {
        name: "closure_move_multi",
        // Each captured upvar produces its own capture arrow on
        // the source side; on the closure side, a single Bind dot
        // enumerates every capture so neither tooltip masks the
        // other (#79 follow-up).
        must_contain: &[
            "Closure capture (move) from s to f",
            "Closure capture (move) from t to f",
            "Closure f captures: s (moved), t (moved)",
            "f goes out of scope. Its captured resources are dropped.",
            "f owns a closure which owns a resource",
        ],
        must_not_contain: &[
            "Move from s to f",
            "Move from t to f",
            // Per-capture closure-side dots suppressed.
            "Closure f captures (moves) s's resource",
            "Closure f captures (moves) t's resource",
            "f is the owner of the resource",
        ],
    },
    TooltipExpect {
        name: "closure_borrow_imm",
        // Non-`move` closure that reads its upvar → static borrow.
        // No move captures, so f's scope-end gets the plain owner
        // OOS message — there are no resources to drop. The borrow
        // returns at f's NLL last use (the call on the line after
        // the closure literal), not at the closure's lexical scope.
        must_contain: &[
            "Move from String::from to s",
            "Closure capture (immutable borrow) from s to f",
            "Closure f captures: s (immutably borrowed)",
            "Return immutably borrowed resource from f to s",
            "f's immutable borrow ends",
            "s's resource is no longer immutably borrowed",
            "f goes out of scope",
            // Borrow-only closure → f owns the closure value but
            // not any captured resource.
            "f owns a closure",
        ],
        must_not_contain: &[
            // Critically: the closure should *not* be modeled as a
            // move — `s` is still usable after `f` runs.
            "Move from s to f",
            "Closure capture (move) from s to f",
            "Immutable borrow from s to f",
            "Closure f captures an immutable reference to s",
            // No move captures → no "captured resources are dropped".
            "f goes out of scope. Its captured resources are dropped.",
            // Borrow-only closure → no upgrade to "owns a resource".
            "f owns a closure which owns a resource",
            "f is the owner of the resource",
        ],
    },
    TooltipExpect {
        name: "closure_borrow_mut",
        // Non-`move` closure that mutates its upvar → mutable borrow.
        // Same as the immutable case for scope-end: no resources
        // are dropped at the closure binding's OOS, and the
        // borrow returns at the closure's NLL last use.
        must_contain: &[
            "Move from String::from to s",
            "Closure capture (mutable borrow) from s to f",
            "Closure f captures: s (mutably borrowed)",
            "Return mutably borrowed resource from f to s",
            "f's mutable borrow ends",
            "s's resource is no longer mutably borrowed",
            "f goes out of scope",
            "f owns a closure",
        ],
        must_not_contain: &[
            "Move from s to f",
            "Closure capture (move) from s to f",
            "Mutable borrow from s to f",
            "Closure f captures a mutable reference to s",
            "f goes out of scope. Its captured resources are dropped.",
            "f owns a closure which owns a resource",
            "f is the owner of the resource",
        ],
    },
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

/// Extract every tooltip text from a rendered timeline panel.
///
/// The renderer attaches `data-tooltip-text="..."` to many element
/// kinds: arrows in `<g id="arrows">`, dots in `<g id="events">`,
/// state-message vertical lines, column labels, function-icon dots,
/// etc. We pull all of them and let the per-snippet expectations
/// pick out the ones that matter — that way a test for "the
/// `len reads from s` interaction" doesn't have to know whether
/// the renderer puts that on a dot vs. an arrow.
///
/// The renderer HTML-escapes the tooltip body and embeds nested
/// `<span>` markup for the variable-name color spans; both get
/// stripped here so comparisons are against the plain visible text.
fn timeline_tooltips(timeline_svg: &str) -> Vec<String> {
    let mut out = Vec::new();
    for chunk in timeline_svg.split("data-tooltip-text=\"").skip(1) {
        let raw = match chunk.find('"') {
            Some(end) => &chunk[..end],
            None => continue,
        };
        let unescaped = raw
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&amp;", "&");
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

#[test]
fn corpus_expected_tooltips_present() {
    let mut failures = Vec::new();
    for expect in EXPECTED_TOOLTIPS {
        let res = std::panic::catch_unwind(|| {
            let rv = run_with_local_backend(expect.name);
            let actual = timeline_tooltips(&rv.timeline_panel_string());

            let missing: Vec<&str> = expect
                .must_contain
                .iter()
                .copied()
                .filter(|want| !actual.iter().any(|a| a == want))
                .collect();
            let unexpected: Vec<&str> = expect
                .must_not_contain
                .iter()
                .copied()
                .filter(|forbid| actual.iter().any(|a| a == forbid))
                .collect();

            if !missing.is_empty() || !unexpected.is_empty() {
                let mut msg = String::new();
                if !missing.is_empty() {
                    msg.push_str(&format!("missing {:?}; ", missing));
                }
                if !unexpected.is_empty() {
                    msg.push_str(&format!("unexpectedly present {:?}; ", unexpected));
                }
                msg.push_str(&format!(
                    "actual tooltips were:\n  {}",
                    actual.join("\n  ")
                ));
                panic!("{}", msg);
            }
        });
        if let Err(payload) = res {
            let msg = payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "<non-string panic>".to_string());
            failures.push(format!("{}: {}", expect.name, msg));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} tooltip assertions failed:\n  {}",
        failures.len(),
        EXPECTED_TOOLTIPS.len(),
        failures.join("\n  ")
    );
}
