//! Render a snippet through the rustviz2 plugin and write the two
//! SVG panels to disk.
//!
//!     cargo run --example render_to_files -- /tmp/out
//!
//! Requires the nightly toolchain (auto-installed by
//! `rust-toolchain.toml` at the workspace root) and `cargo rv-plugin`
//! on `PATH` (see `rustviz2-plugin/`). For a one-shot CLI rather
//! than a code example, use the `rustviz` binary in this crate
//! (`cargo install --path . --bin rustviz`).

use std::{env, fs, path::PathBuf, process};

use rustviz2::Rustviz;

const SNIPPET: &str = r#"
fn main() {
    let s = String::from("hello");
    let t = s;
    println!("{}", t);
}
"#;

fn main() {
    let out_dir = env::args().nth(1).map(PathBuf::from).unwrap_or_else(|| {
        eprintln!("usage: render_to_files <out-dir>");
        process::exit(2);
    });
    fs::create_dir_all(&out_dir).unwrap();

    let rv = Rustviz::new(SNIPPET).expect("plugin invocation failed");
    fs::write(out_dir.join("code.svg"), rv.code_panel_string()).unwrap();
    fs::write(out_dir.join("timeline.svg"), rv.timeline_panel_string()).unwrap();

    println!("wrote {} (height = {}px)", out_dir.display(), rv.height());
}
