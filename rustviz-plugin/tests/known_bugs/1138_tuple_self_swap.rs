// #1138: `t = (t.1, t.0);` says the moved fields' old values are dropped at the assignment.
// Known bug (KNOWN_INVARIANT_FAILURES in rustviz-lib/tests/common/invariants.rs).
fn main() {
    let mut t = (String::from("a"), String::from("b"));
    t = (t.1, t.0);
    println!("{} {}", t.0, t.1);
}
