// #281: a part after the first runs only if the parts before it held.
// `take(s)` moves `s` only when `y` matched, and the else branch also
// runs when `take` returns false, so there `s` may have been moved.

fn take(_s: String) -> bool { true }

fn main() {
    let y = Some(1);
    let s = String::from("a");
    if let Some(n) = y && take(s) {
        println!("{}", n);
    } else {
        println!("no");
    }
}
