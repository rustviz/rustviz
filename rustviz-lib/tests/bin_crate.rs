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
