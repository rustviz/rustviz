// #688: no `else`, and the body moves `s`, so the path that skips the
// body is drawn as its own column. On that path `t.len()` may have run
// (when `o` is `Some`), and its borrow of `t` has ended.

fn take(_s: String) {}

pub fn f(o: Option<i32>) {
    let s = String::from("hi");
    let t = String::from("t");
    if let Some(n) = o && t.len() > 1 {
        take(s);
        println!("{}", n);
    }
    println!("{}", t);
}
