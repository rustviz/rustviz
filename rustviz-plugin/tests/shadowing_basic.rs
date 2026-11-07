// Shadowing: the second `let s = ...` creates a new variable with
// the same name (drawn as `s₂`). The first `s` can't be named any
// more, but it stays alive until the end of `main`, where both
// Strings are dropped.

fn main() {
    let s = String::from("a");
    let s = String::from("b");
    println!("{}", s);
}
