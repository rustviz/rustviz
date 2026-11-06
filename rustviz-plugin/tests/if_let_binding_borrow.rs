// #382: `if let Some(a) = &x`. `a` borrows the String in `x` only when
// the pattern matches, so the borrow is drawn where the if branch's
// column starts, not on the split row above it.
fn show(_r: &String) {}
fn take(_k: i32) {}

fn main() {
    let x = Some(String::from("a"));
    if let Some(a) = &x {
        show(a);
    } else {
        take(0);
    }
}
