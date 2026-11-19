// #1123: `slice = &mut slice[1..]` is drawn as a borrow from slice to itself, ending v's loan.
// Known bug (KNOWN_INVARIANT_FAILURES in rustviz-lib/tests/common/invariants.rs).
fn main() {
    let mut v = vec![1, 2, 3];
    let mut slice: &mut [i32] = &mut v;
    slice = &mut slice[1..];
    slice[0] = 99;
    println!("{:?}", v);
}
