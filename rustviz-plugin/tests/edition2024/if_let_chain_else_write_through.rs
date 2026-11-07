// #688: in `f`, `a` mutably borrows the String in `x`, and the block
// writes through it before `n > 0` can be false. On the way into the
// else branch `a` goes out of scope (ending the borrow), but `x` may
// have been changed through it. In `g` nothing writes through `a`, so
// `x` is only said to have been borrowed.

pub fn f(o: Option<i32>) {
    let mut x = Some(String::from("a"));
    if let Some(n) = o && let Some(a) = &mut x && { a.push('b'); n > 0 } {
        println!("{}", a);
    } else {
        println!("{:?}", x);
    }
}

pub fn g(o: Option<i32>) {
    let mut y = Some(String::from("a"));
    if let Some(n) = o && let Some(b) = &mut y && n > 0 {
        b.push('c');
    } else {
        println!("{:?}", y);
    }
}
