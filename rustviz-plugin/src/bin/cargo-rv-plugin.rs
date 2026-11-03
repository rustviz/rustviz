// The cli binary needs the same rustc_private gate as the lib so its
// transitive rustc_driver dependency can be linked.
#![feature(rustc_private)]

extern crate rustc_driver;

// rustc_plugin >= 0.15 returns the exit code instead of calling exit();
// it must be returned from main, or failures exit 0.
fn main() -> std::process::ExitCode {
  rustc_plugin::cli_main(rustviz_plugin::RVPlugin)
}
