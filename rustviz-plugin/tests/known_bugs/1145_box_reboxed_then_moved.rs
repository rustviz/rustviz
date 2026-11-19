// #1145: `let _s = *b; b = Box::new(..); drop(b);`: b is still freed at scope end.
// Known bug (KNOWN_INVARIANT_FAILURES in rustviz-lib/tests/common/invariants.rs).
fn main() {
    let mut b = Box::new(String::from("a"));
    let _s = *b;
    b = Box::new(String::from("b"));
    drop(b);
}
