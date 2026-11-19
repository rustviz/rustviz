// #1131: `let r = &mut &x;`: an immutable borrow of x is returned as a mutable one.
// Known bug (KNOWN_INVARIANT_FAILURES in rustviz-lib/tests/common/invariants.rs).
fn main() {
    let x = String::from("hi");
    let r = &mut &x;
    println!("{}", r);
}
