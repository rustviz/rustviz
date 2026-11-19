// #1129: `for x in r` (r: `&mut Vec`): x gives back a mutable borrow r never lent.
// Known bug (KNOWN_INVARIANT_FAILURES in rustviz-lib/tests/common/invariants.rs).
fn main() {
    let mut v = vec![1, 2, 3];
    let r = &mut v;
    for x in r {
        *x += 1;
    }
}
