// #1132: a mutable borrow in one arm, `drop(s)` in the other: s is drawn as owner after the join.
// Known bug (KNOWN_INVARIANT_FAILURES in rustviz-lib/tests/common/invariants.rs).
fn demo(cond: bool) {
    let mut s = String::from("hello");
    let mut other = String::from("other");
    let mut r = &mut other;
    if cond {
        r = &mut s;
    } else {
        drop(s);
    }
    r.push_str("!");
    println!("{}", other);
}
fn main() { demo(true); }
