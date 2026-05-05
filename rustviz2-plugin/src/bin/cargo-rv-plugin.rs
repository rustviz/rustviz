// The cli binary needs the same rustc_private gate as the lib so its
// transitive rustc_driver dependency can be linked.
#![feature(rustc_private)]

extern crate rustc_driver;

fn main() {
  rustc_plugin::cli_main(rustviz2_plugin::RVPlugin);
}
