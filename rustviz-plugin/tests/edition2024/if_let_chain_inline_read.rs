// #688: no `else` and the body moves nothing, so the `if` is drawn
// inline, with `t.len()`'s borrow on `t`'s one column. A borrow that
// ended in the condition doesn't make a second column for the path that
// skips the body.

pub fn f(o: Option<i32>) {
    let t = String::from("t");
    if let Some(n) = o && t.len() > 1 {
        println!("{}", n);
    }
    println!("{}", t);
}
