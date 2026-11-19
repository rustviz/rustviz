// #1152: `let it = r.iter_mut();` (r: `&mut Vec`): it's mutable borrow ends as an immutable one.
// Known bug (KNOWN_INVARIANT_FAILURES in rustviz-lib/tests/common/invariants.rs).
fn main() {
    let mut v = vec![1, 2, 3];
    let r = &mut v;
    let mut it = r.iter_mut();
    let _x = it.next();
    r.push(4);
}
