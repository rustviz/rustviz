// Shadowing doesn't end the first variable (#173): the first `s` is
// still borrowed by `r` after the second `let s`, and it's dropped at
// the end of `main`, not where it's shadowed.

fn main() {
    let s = String::from("hello");
    let r = &s;
    let s = String::from("world");
    println!("{} {}", r, s);
}
