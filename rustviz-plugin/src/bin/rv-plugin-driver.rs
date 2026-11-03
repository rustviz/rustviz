// The driver binary needs the same rustc_private gate + extern crate
// declarations as the lib so the rustc_driver link succeeds.
#![feature(rustc_private)]

extern crate rustc_driver;
extern crate rustc_interface;

// rustc_plugin >= 0.15 returns the exit code instead of calling exit();
// it must be returned from main, or failures exit 0.
fn main() -> std::process::ExitCode {
  rustc_plugin::driver_main(rustviz_plugin::RVPlugin)
}
