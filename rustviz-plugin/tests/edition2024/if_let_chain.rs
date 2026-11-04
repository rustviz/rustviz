// `if let … && let …` (#198): both lets' bindings get columns, with the
// borrow of `x` and the copy out of `y` drawn in the if branch. Before
// the fix neither `a` nor `b` appeared, and their uses were dropped.

fn show(_r: &String) {}
fn take(_k: i32) {}

fn main() {
    let x = Some(String::from("a"));
    let y = Some(2);
    if let Some(a) = &x && let Some(b) = y {
        show(a);
        take(b);
    } else {
        take(0);
    }
}
