// Since Rust 2021 a closure captures only the places it uses, and that
// includes tuple elements: `move || t.0.len()` moves `t.0` into `f` and
// leaves `t.1` with `t`, so `t.1` can still be used and is dropped at the
// end of main. `|| u.1.len()` borrows only `u.1`.
fn main() {
    let t = (String::from("a"), String::from("b"));
    let f = move || t.0.len();
    println!("{} {}", f(), t.1);
    let u = (String::from("c"), String::from("d"));
    let g = || u.1.len();
    println!("{}", g());
    let r = u.0;
    println!("{}", r);
}
