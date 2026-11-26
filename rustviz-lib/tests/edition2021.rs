//! Programs whose ownership facts depend on the edition, rendered in an
//! edition-2021 crate (the corpus fixtures render in 2024, `cargo new`'s
//! default). Runs the installed `cargo rv-plugin`.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use tempfile::tempdir;

fn timeline_2021(code: &str) -> String {
    let dir = tempdir().unwrap();
    let dir = dir.path();
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("Cargo.toml"),
        "[package]\nname = \"user-code\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    let toolchain = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("rust-toolchain.toml");
    fs::copy(toolchain, dir.join("rust-toolchain.toml")).unwrap();
    fs::write(dir.join("src").join("lib.rs"), code).unwrap();
    let out = Command::new("cargo")
        .arg("rv-plugin")
        .env("CARGO_TERM_COLOR", "never")
        .env_remove("CARGO_TARGET_DIR")
        .current_dir(dir)
        .output()
        .expect("run cargo rv-plugin");
    assert!(out.status.success(), "plugin failed:\n{}", String::from_utf8_lossy(&out.stderr));
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).expect("plugin JSON");
    json["timeline_panel"].as_str().expect("timeline_panel").to_string()
}

/// The tooltip texts of `var`'s marks of `kind` (`dot`, `line`) tagged
/// with source row `line`.
fn tooltips(svg: &str, var: &str, kind: &str, line: usize) -> Vec<String> {
    let doc = roxmltree::Document::parse(svg).unwrap();
    doc.descendants()
        .filter(|n| {
            n.attribute("data-rv-var") == Some(var)
                && n.attribute("data-rv-kind") == Some(kind)
                && n.attribute("data-rv-line").and_then(|l| l.parse().ok()) == Some(line)
        })
        .filter_map(|n| n.attribute("data-tooltip-text").map(str::to_string))
        .collect()
}

/// Before edition 2024 an `if let` scrutinee's temporaries live through the
/// `else` block: `G(&v)` (with `Drop`) keeps v borrowed there (rustc: `&mut v`
/// in the `else` is E0502 in 2021, fine in 2024). #1777's failure-arm end for
/// a pattern borrow must not give v back at the `else` (Codex review of #1777).
#[test]
fn if_let_guard_temporary_keeps_loan_through_else_2021() {
    let code = r#"struct G<'a>(&'a Vec<String>);
impl Drop for G<'_> { fn drop(&mut self) {} }
impl<'a> G<'a> { fn get(&self) -> Option<&'a String> { self.0.first() } }
pub fn main() {
    let v = vec![String::from("a")];
    if let Some(x) = G(&v).get() {
        println!("{}", x);
    } else {
        println!("none");
    }
    println!("{}", v.len());
}
"#;
    let svg = timeline_2021(code);
    let at_else = tooltips(&svg, "v", "dot", 8);
    assert!(
        at_else.iter().all(|t| !t.contains("no longer immutably borrowed")),
        "v given back at the else row (line 8) in edition 2021: {:?}",
        at_else
    );
    // The guard is dropped at the end of the `if let` (line 10): v is its
    // owner again from there.
    let after = tooltips(&svg, "v", "line", 10);
    assert!(
        !after.is_empty() && after.iter().all(|t| t.contains("is the owner of the resource")),
        "v after the if let (line 10) in edition 2021: {:?}",
        after
    );
}
