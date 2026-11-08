// #636 with a chain that moves first: `let Some(a) = x` moves the
// String out of `x` into `a`, then `check(&a)?` may return from `f`.
// On that early return `f` drops `w`'s String and `a`'s (the one taken
// from `x`); `x` no longer holds a String, so nothing of it is dropped.

fn check(s: &str) -> Option<bool> {
    if s.is_empty() { None } else { Some(true) }
}

fn f(x: Option<String>) -> Option<usize> {
    let w = String::from("w");
    if let Some(a) = x && check(&a)? {
        println!("{} {}", a, w);
    } else {
        println!("{}", w);
    }
    Some(1)
}

fn main() {
    f(Some(String::from("hi")));
}
