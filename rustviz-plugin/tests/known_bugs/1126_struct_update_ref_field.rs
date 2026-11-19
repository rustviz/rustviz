// #1126: `S { x: 2, ..s1 }` with a reference field: s2.r has no line, and its borrow ends on an empty column.
// Known bug (KNOWN_INVARIANT_FAILURES in rustviz-lib/tests/common/invariants.rs).
struct S<'a> { x: i32, r: &'a mut String }
fn main() {
    let mut a = String::from("a");
    let s1 = S { x: 1, r: &mut a };
    let s2 = S { x: 2, ..s1 };
    let _spacer = 1;
    s2.r.push('!');
}
