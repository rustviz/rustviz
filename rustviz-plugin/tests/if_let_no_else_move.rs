// #257: `if let` without `else`. Its body moves `s`, and the pattern
// moves the String out of `opt`; both happen only when `opt` is
// `Some`.
fn take(_s: String) {}

fn main() {
    let opt = Some(String::from("a"));
    let s = String::from("b");
    if let Some(x) = opt {
        take(x);
        take(s);
    }
}
