// #1127: `drop(e)` on a struct holding a reference: e.part's line goes on after e is moved.
// Known bug (KNOWN_INVARIANT_FAILURES in rustviz-lib/tests/common/invariants.rs).
struct Excerpt<'a> { part: &'a str }
impl<'a> Excerpt<'a> {
    fn part(&self) -> &'a str { self.part }
}
fn main() {
    let novel = String::from("Call me Ishmael.");
    let e = Excerpt { part: &novel };
    let p = e.part();
    drop(e);
    println!("{}", p);
}
