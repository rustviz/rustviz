// #636: a `?` in a later part of an `if let` chain with an `else`. When
// `check(a)` is `None`, the `?` returns from `f` before either branch
// runs, and `f` drops what it owns there: `w`'s String and `x`'s.
// (`a` is a reference: nothing to drop.) The `?` runs only once
// `let Some(a) = &x` has matched, and like the chain's other later
// parts it's drawn in the if branch, on the `if` line. Before, the
// branch's columns dropped the `?`'s events, so no tooltip said so.

fn check(s: &str) -> Option<bool> {
    if s.is_empty() { None } else { Some(true) }
}

fn f(x: Option<String>) -> Option<usize> {
    let w = String::from("w");
    if let Some(a) = &x && check(a)? {
        println!("{} {}", a, w);
    } else {
        println!("{}", w);
    }
    Some(1)
}

fn main() {
    f(Some(String::from("hi")));
}
