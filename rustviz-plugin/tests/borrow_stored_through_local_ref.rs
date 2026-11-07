// #512: `rh.r = &t;` writes the field `hold.r` through the local
// reference `rh` (`let rh = &mut hold;`). It used to draw nothing: the
// borrow of t was lost because rh's borrow of hold ends on the same
// line. Now t is "immutably borrowed by hold.r" from line 18 until
// hold's last use (line 19). s's borrow also lasts until line 19:
// hold.r no longer points to s after the write, but hold's type has a
// single lifetime for both borrows, so the borrow checker keeps s
// borrowed while hold is in use (a `&mut s` between lines 18 and 19
// would not compile).
struct H<'a> { r: &'a String }
struct O<'a> { inner: H<'a>, n: i32 }

fn main() {
    let s = String::from("s");
    let t = String::from("t");
    let mut hold = H { r: &s };
    let rh = &mut hold;
    rh.r = &t;
    println!("{}", hold.r);
}

// A nested field, written through an explicit dereference.
fn nested() {
    let a = String::from("a");
    let b = String::from("b");
    let mut o = O { inner: H { r: &a }, n: 1 };
    let ri = &mut o.inner;
    (*ri).r = &b;
    println!("{} {}", o.inner.r, o.n);
}
