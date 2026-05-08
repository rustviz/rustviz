// Regression for: a function call inside `assert!(…)` didn't surface
// its PassByStaticReference event, so the timeline showed the
// borrow's lifetime ending but no `f` icon at the call site —
// inconsistent with an equivalent free-fn call in the same example.
//
// `assert!(cond)` lowers to `match cond { true => {}, _ => panic!(…) }`.
// `descend_through_expansion` skipped the whole Match (to avoid the
// synthetic panic-arm), but that also dropped the user-written
// scrutinee. Now it descends into the scrutinee while still skipping
// the arms.
fn pred(_a: &i32, _b: &i32) -> bool { true }

fn main() {
    let n = 1;
    let m = 2;
    let r1 = &n;
    let r2 = &m;
    assert!(pred(r1, r2));
}
