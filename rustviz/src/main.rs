//! `cargo install rustviz` installs this binary: the RustViz CLI
//! (`rustviz svg`, `rustviz html`, `rustviz init`), implemented in
//! `rustviz-cli` and shared with it.
fn main() -> anyhow::Result<()> {
    rustviz_cli::run()
}
