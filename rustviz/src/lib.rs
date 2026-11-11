//! `rustviz` — RustViz 2, ownership and borrow timelines for Rust.
//!
//! This crate is the short, discoverable name for RustViz:
//!
//! - `cargo install rustviz` installs the `rustviz` command-line tool
//!   (`rustviz svg foo.rs`, `rustviz html foo.rs`, `rustviz init`).
//! - `rustviz = "2"` in `Cargo.toml` gives the library API,
//!   re-exported from [`rustviz-lib`](https://crates.io/crates/rustviz-lib):
//!   `rustviz::Rustviz::new(code)` returns the SVG panels.
//!
//! Rendering runs the `rustviz-plugin` rustc plugin out of process,
//! which needs a pinned nightly toolchain; `rustviz init` installs both.

pub use rustviz_lib::*;
