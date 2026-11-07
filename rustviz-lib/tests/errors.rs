//! End-to-end checks of what a student sees when RustViz can't produce a
//! visualization: the program doesn't compile, or the plugin hits a
//! construct it can't handle yet.
//!
//! Like `corpus.rs`, these run the *installed* plugin through the
//! `RV_RUNNER=local` backend; reinstall it after plugin changes
//! (`cargo install --locked --path rustviz-plugin`).

use std::env;
use std::fs;
use std::process::{Command, Stdio};

use rustviz_lib::{error_kind, ErrorKind, Rustviz, DOES_NOT_COMPILE_INTRO, UNSUPPORTED_PREFIX};

/// Snippets the plugin refuses on `main` (it panics, which surfaces as
/// "RustViz can't visualize this program yet"). The tests only need
/// *one* of them to still be refused; one that starts visualizing is
/// skipped. If they all start working, add a new one here.
///
/// The first is a deliberate refusal, not a bug: a match guard that
/// moves a variable declared outside it can't be drawn truthfully with
/// one column per arm (#247, #248), so it stays refused until that
/// design work is done. The second is an ordinary crash (#210).
const KNOWN_PANICKING: &[(&str, &str)] = &[
    // A match guard that moves an outer variable (#247).
    (
        "guard_moves_outer",
        "fn consume(s: String) -> bool {\n    s.len() > 3\n}\nfn main() {\n    let s = String::from(\"hello\");\n    let opt = Some(5);\n    match opt {\n        Some(_) if consume(s) => println!(\"long\"),\n        _ => println!(\"other\"),\n    }\n}\n",
    ),
    // Calling a closure with a `&mut` argument (#210).
    (
        "closure_mutref_arg",
        "fn main() {\n    let mut s = String::from(\"a\");\n    let mut push = |q: &mut String| q.push('b');\n    push(&mut s);\n    println!(\"{}\", s);\n}\n",
    ),
];

/// The Rust Book's "two mutable borrows" example (ch04-02).
const BOOK_TWO_MUT_BORROWS: &str = r#"fn main() {
    let mut s = String::from("hello");

    let r1 = &mut s;
    let r2 = &mut s;

    println!("{}, {}", r1, r2);
}
"#;

const TYPE_ERROR: &str = r#"fn main() {
    let x: i32 = "hello";
    println!("{}", x);
}
"#;

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
         before `cargo test`."
    );
}

fn render(src: &str) -> Result<Rustviz, Box<dyn std::error::Error>> {
    ensure_plugin_installed();
    env::set_var("RV_RUNNER", "local");
    Rustviz::new(src)
}

/// Run `cargo rv-plugin` directly on `src` (what `rustviz-lib`'s local
/// backend does) and return (success, stderr).
fn run_plugin_raw(src: &str) -> (bool, String) {
    ensure_plugin_installed();
    let dir = tempfile::tempdir().unwrap();
    let crate_dir = dir.path().join("user-code");
    fs::create_dir_all(crate_dir.join("src")).unwrap();
    fs::write(
        crate_dir.join("Cargo.toml"),
        "[package]\nname = \"user-code\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    fs::write(crate_dir.join("rust-toolchain.toml"), rustviz_lib::TOOLCHAIN).unwrap();
    fs::write(crate_dir.join("src").join("lib.rs"), src).unwrap();
    let out = Command::new("cargo")
        .arg("rv-plugin")
        .current_dir(&crate_dir)
        // The developer escape hatches restore the full panic output.
        .env_remove("RUST_BACKTRACE")
        // CI sets CARGO_TERM_COLOR=always; rustviz-lib forces "never" for
        // user-facing output, so do the same here.
        .env("CARGO_TERM_COLOR", "never")
        .env_remove("RUSTVIZ_DEBUG")
        .output()
        .unwrap();
    (out.status.success(), String::from_utf8_lossy(&out.stderr).into_owned())
}

#[test]
fn plugin_panic_prints_one_clean_unsupported_line() {
    let mut panicked = 0;
    for (name, src) in KNOWN_PANICKING {
        let (ok, stderr) = run_plugin_raw(src);
        if ok {
            eprintln!("{} no longer panics the plugin; skipping", name);
            continue;
        }
        panicked += 1;
        let lines: Vec<&str> = stderr.lines().filter(|l| l.contains(UNSUPPORTED_PREFIX)).collect();
        assert_eq!(lines.len(), 1, "{}: expected one unsupported line in:\n{}", name, stderr);
        let line = lines[0];
        assert!(
            line.contains("RustViz can't visualize this program yet (internal error at ")
                && line.contains(".rs:")
                && line.ends_with(
                    "Please report it at https://github.com/rustviz/rustviz/issues with your snippet."
                ),
            "{}: unexpected line {:?}",
            name,
            line
        );
        for noise in ["panicked at", "RUST_BACKTRACE", "Caused by", "process didn't exit", "rv-plugin-driver"] {
            assert!(!stderr.contains(noise), "{}: {:?} leaked into stderr:\n{}", name, noise, stderr);
        }

        // ...and the library turns it into an Unsupported error.
        let err = render(src).expect_err("known-panicking snippet rendered");
        assert_eq!(error_kind(err.as_ref()), ErrorKind::Unsupported, "{}: {}", name, err);
        assert!(err.to_string().starts_with("RustViz can't visualize this program yet"), "{}", err);
    }
    assert!(
        panicked > 0,
        "none of KNOWN_PANICKING panics the plugin any more; add a snippet that does"
    );
}

/// Snippets that trip an internal-state guard, with the reason the
/// student-facing line must give (#134). The reason must be readable: no
/// bare "not possible", and no std `unwrap()` text (a missing lookup says
/// so in words). As with `KNOWN_PANICKING`, a snippet that starts working
/// is skipped, but at least one must still trip a guard.
const GUARD_REASONS: &[(&str, &str, &str)] = &[
    // A match guard that moves an outer variable (#247). (Shadowing a
    // moved variable (#253) used to be the example here; it renders now.)
    (
        "guard_moves_outer",
        "fn consume(s: String) -> bool { s.len() > 3 }\nfn main() {\n    let s = String::from(\"hello\");\n    let opt = Some(5);\n    match opt {\n        Some(_) if consume(s) => println!(\"yes\"),\n        _ => println!(\"no\"),\n    }\n}\n",
        "match guards that move or assign a variable declared outside the guard",
    ),
    // A write through `*r` in both arms of an if/else (#264): the layout
    // has no column for the event's timeline.
    (
        "write_through_ref_in_arms",
        "fn main() {\n    let mut s = String::from(\"a\");\n    let t = String::from(\"b\");\n    let c = true;\n    let r = &mut s;\n    if c {\n        *r = t;\n    } else {\n        *r = String::new();\n    }\n}\n",
        "no layout column for the timeline of event",
    ),
];

#[test]
fn plugin_internal_errors_give_a_readable_reason() {
    let mut tripped = 0;
    for (name, src, reason) in GUARD_REASONS {
        let (ok, stderr) = run_plugin_raw(src);
        if ok {
            eprintln!("{} no longer trips an internal guard; skipping", name);
            continue;
        }
        tripped += 1;
        let line = stderr
            .lines()
            .find(|l| l.contains(UNSUPPORTED_PREFIX))
            .unwrap_or_else(|| panic!("{}: no unsupported line in:\n{}", name, stderr));
        assert!(line.contains("(internal error at "), "{}: {:?}", name, line);
        assert!(line.contains(reason), "{}: reason {:?} missing from {:?}", name, reason, line);
        for bad in ["not possible", "Not possible", "Option::unwrap", "Result::unwrap", "internal state:", "panicked at"] {
            assert!(!stderr.contains(bad), "{}: {:?} leaked into:\n{}", name, bad, stderr);
        }
        let err = render(src).expect_err("guard snippet rendered");
        assert_eq!(error_kind(err.as_ref()), ErrorKind::Unsupported, "{}: {}", name, err);
    }
    assert!(tripped > 0, "none of GUARD_REASONS trips a guard any more; add a snippet that does");
}

#[test]
fn book_borrow_error_reads_as_does_not_compile() {
    let err = render(BOOK_TWO_MUT_BORROWS).expect_err("borrow error rendered");
    assert_eq!(error_kind(err.as_ref()), ErrorKind::DoesNotCompile, "{}", err);
    let shown = err.to_string();
    let diagnostics = shown
        .strip_prefix(DOES_NOT_COMPILE_INTRO)
        .and_then(|d| d.strip_prefix("\n\n"))
        .unwrap_or_else(|| panic!("missing intro:\n{}", shown));
    assert!(
        diagnostics.starts_with("error[E0499]: cannot borrow `s` as mutable more than once at a time\n --> your code:5:14"),
        "{}",
        diagnostics
    );
    for noise in ["-Ztrack-diagnostics", "could not compile", "src/lib.rs", "/tmp", "/var/"] {
        assert!(!shown.contains(noise), "{:?} leaked into:\n{}", noise, shown);
    }
}

#[test]
fn type_error_reads_as_does_not_compile() {
    let err = render(TYPE_ERROR).expect_err("type error rendered");
    assert_eq!(error_kind(err.as_ref()), ErrorKind::DoesNotCompile, "{}", err);
    assert!(err.to_string().contains("error[E0308]: mismatched types\n --> your code:2:18"), "{}", err);
}
