//! `cargo rv-plugin` on a bin crate (`src/main.rs`, the `cargo new` default)
//! must draw exactly what it draws for the same code in a lib crate (#752).
//! It used to read a hard-coded `src/lib.rs`, found nothing, and panicked.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::tempdir;

fn run_plugin_on(dir: &Path, file: &str, code: &str) -> std::process::Output {
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("Cargo.toml"),
        "[package]\nname = \"user-code\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    let toolchain = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("rust-toolchain.toml");
    fs::copy(toolchain, dir.join("rust-toolchain.toml")).unwrap();
    fs::write(dir.join("src").join(file), code).unwrap();
    Command::new("cargo")
        .arg("rv-plugin")
        .env("CARGO_TERM_COLOR", "never")
        .current_dir(dir)
        .output()
        .expect("run cargo rv-plugin")
}

#[test]
fn bin_crate_renders_like_lib_crate() {
    let fixtures = ["fn main() {\n    let s = String::from(\"hi\");\n    let t = s;\n}\n"];
    // Plus a real corpus fixture with a `main` and helper fns.
    let corpus = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("rustviz-plugin")
        .join("tests")
        .join("mutableReferences.rs");
    let corpus_src = fs::read_to_string(corpus).unwrap();
    for code in fixtures.iter().copied().chain(std::iter::once(corpus_src.as_str())) {
        let lib_dir = tempdir().unwrap();
        let bin_dir = tempdir().unwrap();
        let lib = run_plugin_on(lib_dir.path(), "lib.rs", code);
        let bin = run_plugin_on(bin_dir.path(), "main.rs", code);
        assert!(
            bin.status.success(),
            "bin crate failed:\n{}",
            String::from_utf8_lossy(&bin.stderr)
        );
        assert!(lib.status.success());
        assert_eq!(
            String::from_utf8_lossy(&lib.stdout),
            String::from_utf8_lossy(&bin.stdout),
            "bin and lib layouts must render identically"
        );
    }
}

/// A bin crate with `mod shapes;`: `main.rs` and `shapes.rs` each have a
/// function with a distinctive local.
fn run_module_crate(dir: &Path, extra: &[&str]) -> std::process::Output {
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("Cargo.toml"),
        "[package]\nname = \"user-code\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    let toolchain = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("rust-toolchain.toml");
    fs::copy(toolchain, dir.join("rust-toolchain.toml")).unwrap();
    fs::write(
        dir.join("src/main.rs"),
        "mod shapes;\n\nfn main() {\n    let in_main = String::from(\"m\");\n    let moved_main = in_main;\n    shapes::area();\n}\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/shapes.rs"),
        "pub fn area() {\n    let in_shapes = String::from(\"s\");\n    let moved_shapes = in_shapes;\n}\n",
    )
    .unwrap();
    Command::new("cargo")
        .arg("rv-plugin")
        .args(extra)
        .env("CARGO_TERM_COLOR", "never")
        .current_dir(dir)
        .output()
        .expect("run cargo rv-plugin")
}

#[test]
fn file_flag_picks_a_module_file() {
    let dir = tempdir().unwrap();
    let out = run_module_crate(dir.path(), &["--file", "src/shapes.rs"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("moved_shapes"), "shapes.rs's fn is drawn");
    assert!(stdout.contains("in_shapes"));
    assert!(!stdout.contains("moved_main"), "main.rs's fn is not drawn");
    assert!(!stdout.contains("in_main"));
}

#[test]
fn no_flag_draws_the_crate_root_only() {
    let dir = tempdir().unwrap();
    let out = run_module_crate(dir.path(), &[]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("moved_main"));
    assert!(!stdout.contains("moved_shapes"), "module file's fn is not drawn by default");
}

#[test]
fn file_flag_rejects_a_file_the_crate_does_not_compile() {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    // Exists on disk but no `mod nope;` declares it.
    let out = {
        let o = run_module_crate(dir.path(), &["--file", "src/nope.rs"]);
        fs::write(dir.path().join("src/nope.rs"), "fn x() {}\n").unwrap();
        let o2 = Command::new("cargo")
            .args(["rv-plugin", "--file", "src/nope.rs"])
            .env("CARGO_TERM_COLOR", "never")
            .current_dir(dir.path())
            .output()
            .unwrap();
        (o, o2)
    };
    for o in [out.0, out.1] {
        assert!(!o.status.success(), "must exit non-zero");
        let err = String::from_utf8_lossy(&o.stderr);
        assert!(
            err.contains("src/nope.rs isn't compiled as part of this crate (check its `mod` declaration)"),
            "stderr was: {err}"
        );
        assert!(!err.contains("panicked"));
    }
}

#[test]
fn changing_the_file_flag_re_renders_in_the_same_crate_dir() {
    // Cargo caches the check; a different `--file` must still re-run the plugin.
    let dir = tempdir().unwrap();
    let root = run_module_crate(dir.path(), &[]);
    assert!(String::from_utf8_lossy(&root.stdout).contains("moved_main"));
    let shapes = run_module_crate(dir.path(), &["--file", "src/shapes.rs"]);
    let stdout = String::from_utf8_lossy(&shapes.stdout);
    assert!(stdout.contains("moved_shapes"), "stale cached run: {stdout}");
    assert!(!stdout.contains("moved_main"));
}
