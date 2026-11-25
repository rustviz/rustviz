// #1556: `r = r;` with r a `&mut` is a reborrow (`r = &mut *r;`, an
// assignment being a coercion site): r still points to s, and s stays
// mutably borrowed until r's last use. main drew r lending to itself
// ("Mutable reborrow from r to r") and ended r's borrow twice.
#![allow(clippy::self_assignment)]
struct P<'a> {
    r: &'a mut String,
}

fn main() {
    let mut s = String::from("ab");
    let mut r = &mut s;
    r = r;
    r.push('c');
    println!("{}", s);
    field();
}

// A reference field assigned itself: nothing to draw on its column, and
// the loan ends once, at its last use.
fn field() {
    let mut t = String::from("ab");
    let mut p = P { r: &mut t };
    p.r = p.r;
    p.r.push('c');
    println!("{}", t);
}
