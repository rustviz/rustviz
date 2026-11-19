// #1128: `let r2 = id(r);` moves the `&mut` r, but r's borrow still ends later.
// Known bug (KNOWN_INVARIANT_FAILURES in rustviz-lib/tests/common/invariants.rs).
fn id<T>(x: T) -> T { x }
fn main() {
    let mut s = String::from("hello");
    let r = &mut s;
    let r2 = id(r);
    r2.push('!');
}
