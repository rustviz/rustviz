// #688: `s.len()` and `s.contains(..)` borrow `s`, and `r.len()` the
// String `r` points to, in parts that run before a later part can be
// false. Those borrows end when the calls return, before the else branch
// runs; the else column says that they may have happened.

pub fn f(o: Option<usize>, r: &String) {
    let s = String::from("hello");
    if let Some(n) = o && s.len() > n && s.contains('e') && r.len() > n {
        println!("{} {}", s, r);
    } else {
        println!("{} {}", s, r);
    }
}
