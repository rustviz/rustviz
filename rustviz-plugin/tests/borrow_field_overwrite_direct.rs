// #531: `hold.r = &t;` overwrites a reference field directly. The field
// no longer points to s, but `hold`'s type has a single lifetime for its
// reference, so the borrow checker keeps s borrowed while `hold` is in
// use: a `&mut s` between the write and the last use of `hold` is E0502.
// RustViz draws the borrow where the borrow checker ends it: s's borrow
// ends at hold's last use (the println!), on the same row as t's, not at
// the write. (The same write through a local `&mut` is
// borrow_stored_through_local_ref.rs.)
struct H<'a> { r: &'a String }
struct O<'a> { inner: H<'a>, n: i32 }

fn main() {
    let s = String::from("s");
    let t = String::from("t");
    let mut hold = H { r: &s };
    hold.r = &t;
    println!("{}", hold.r);
}

// A nested field.
fn nested() {
    let a = String::from("a");
    let b = String::from("b");
    let mut o = O { inner: H { r: &a }, n: 1 };
    o.inner.r = &b;
    println!("{} {}", o.inner.r, o.n);
}

// A reference *variable* is dead between the two assignments, so there
// the borrow of x really does end at the reassignment.
fn variable() {
    let x = String::from("x");
    let y = String::from("y");
    let mut r = &x;
    println!("{}", r);
    r = &y;
    println!("{}", r);
}
