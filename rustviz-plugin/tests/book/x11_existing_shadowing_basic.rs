// Shadowing: the second `let s = ...` creates a new binding with
// the same name. The first `s` is hidden, not dropped: both Strings
// live until the end of main (copy of tests/shadowing_basic.rs).

fn main() {
    let s = String::from("a");
    let s = String::from("b");
    println!("{}", s);
}
