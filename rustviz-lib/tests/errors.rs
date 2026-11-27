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

// Historical error cases now render: calling a closure with a &mut argument
// (#210), writing through a reference in both arms (#264), shadowing a
// moved binding (#253), moving in a guard (#247), and repointing an outer
// reference in a guard outside a loop (#603).

/// Active refusal. A guard in a loop assigns a new reference to an outer
/// binding. Loans from an iteration can reach the guard again, and the
/// renderer does not work out where those loans end yet.
const GUARD_ASSIGNS_REFERENCE_IN_LOOP: &str = r#"fn main() {
    let s = String::from("a");
    let t = String::from("b");
    let mut r = &t;
    for n in 0..2 {
        match Some(n) {
            Some(_) if { r = &s; r.len() > 3 } => println!("{}", r),
            _ => println!("{}", r),
        }
    }
}
"#;

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
    let (ok, stderr) = run_plugin_raw(GUARD_ASSIGNS_REFERENCE_IN_LOOP);
    assert!(!ok, "the loop guard now renders; update the active error fixture");
    let lines: Vec<&str> = stderr.lines().filter(|l| l.contains(UNSUPPORTED_PREFIX)).collect();
    assert_eq!(lines.len(), 1, "expected one unsupported line in:\n{stderr}");
    let line = lines[0];
    assert!(
        line.contains("RustViz can't visualize this program yet (internal error at ")
            && line.contains(".rs:")
            && line.ends_with(
                "Please report it at https://github.com/rustviz/rustviz/issues with your snippet."
            ),
        "unexpected line {line:?}"
    );
    for noise in ["panicked at", "RUST_BACKTRACE", "Caused by", "process didn't exit", "rv-plugin-driver"] {
        assert!(!stderr.contains(noise), "{noise:?} leaked into stderr:\n{stderr}");
    }

    let err = render(GUARD_ASSIGNS_REFERENCE_IN_LOOP).expect_err("the loop guard now renders");
    assert_eq!(error_kind(err.as_ref()), ErrorKind::Unsupported, "{err}");
    assert!(err.to_string().starts_with("RustViz can't visualize this program yet"), "{err}");
}

#[test]
fn plugin_internal_errors_give_a_readable_reason() {
    let (ok, stderr) = run_plugin_raw(GUARD_ASSIGNS_REFERENCE_IN_LOOP);
    assert!(!ok, "the loop guard now renders; update the active error fixture");
    let line = stderr
        .lines()
        .find(|l| l.contains(UNSUPPORTED_PREFIX))
        .unwrap_or_else(|| panic!("no unsupported line in:\n{stderr}"));
    let reason = "match guards that assign a new reference to a variable declared outside the guard";
    assert!(line.contains(reason), "reason {reason:?} missing from {line:?}");
    // The panic reason is part of the student-facing diagnostic (#134).
    for bad in ["not possible", "Not possible", "Option::unwrap", "Result::unwrap", "internal state:", "panicked at"] {
        assert!(!stderr.contains(bad), "{bad:?} leaked into:\n{stderr}");
    }
    let err = render(GUARD_ASSIGNS_REFERENCE_IN_LOOP).expect_err("the loop guard now renders");
    assert_eq!(error_kind(err.as_ref()), ErrorKind::Unsupported, "{err}");
    assert!(err.to_string().contains(reason), "{err}");
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
