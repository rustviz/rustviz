// #257: both branches of the inner `if` move `s`, but the outer `if`
// has no `else`, so `s` is still maybe-moved after it.
fn take(_s: String) {}

fn main() {
    let s = String::from("a");
    let a = true;
    let b = false;
    if a {
        if b {
            take(s);
        } else {
            drop(s);
        }
    }
}
