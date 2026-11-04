// A closure's body runs at each call, not where the closure is
// written (#133). `s` is read by the body at both `f()` lines; the
// capture borrow taken at `let f` ends after the last call.

fn main() {
    let s = String::from("hi");
    let f = || println!("{}", s);
    f();
    f();
    println!("{}", s);
}
