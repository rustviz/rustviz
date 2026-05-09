// `match pair { (x, y) => … }` — tuple pattern over a single
// tuple-typed scrutinee. Pre-fix this panicked with an out-of-
// bounds `parents[i]` because the destructure code assumed
// parents.len() == pat_list.len() (the guard-was-a-literal-tuple
// shape, `match (a, b) { (x, y) => … }`). When parents has length
// 1 and pat_list has more, every inner pattern destructures out of
// the same single parent.
//
// Single-arm match too — same inlining as the other single-arm
// conditionals.

fn show(_s: &String) {} // rustviz: hide

fn main() {
    let pair = (String::from("a"), String::from("b"));
    match pair {
        (x, y) => {
            show(&x);
            show(&y);
        }
    }
}
