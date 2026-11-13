//! `rustviz` binary installed by `cargo install rustviz-cli`. The CLI
//! lives in the library half of this crate so the `rustviz` crate's
//! binary can share it. Prefer `cargo install rustviz`.
fn main() -> anyhow::Result<()> {
    rustviz_cli::run()
}
