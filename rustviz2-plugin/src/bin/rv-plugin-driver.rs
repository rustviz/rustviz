// The driver binary needs the same rustc_private gate + extern crate
// declarations as the lib so the rustc_driver link succeeds.
#![feature(rustc_private)]

extern crate rustc_driver;
extern crate rustc_interface;

fn main() {
  rustc_plugin::driver_main(rustviz2_plugin::RVPlugin);
}
