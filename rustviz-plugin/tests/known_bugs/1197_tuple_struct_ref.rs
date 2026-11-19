// #1197: a tuple-struct constructor holding a reference draws no borrow; h.0 has no line.
// Known bug (KNOWN_INVARIANT_FAILURES in rustviz-lib/tests/common/invariants.rs).
struct Holder<'a>(&'a String);
fn main() {
    let x = String::from("x");
    let h = Holder(&x);
    println!("{}", h.0);
}
