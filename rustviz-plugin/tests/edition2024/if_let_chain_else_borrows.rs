// #688: when `x` is `Some`, `a` borrows from `x` and `y.take()` runs
// (mutably borrowing `y`, and leaving `None` in it) before the second
// `let` can fail. Then the else branch runs: `a` has gone out of scope
// and `take` has returned, so neither borrow is live, but `y` may have
// been changed. The else column started with `x` and `y` untouched.

fn main() {
    let x = Some(String::from("a"));
    let mut y: Option<i32> = None;
    if let Some(a) = &x && let Some(b) = y.take() {
        println!("{} {}", a, b);
    } else {
        println!("{:?}", y);
    }
}
