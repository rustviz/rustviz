// A reference stored through a local `&mut` into a place that has a
// column, or into a value that holds borrows: `rh.r = &t` (rh = &mut
// hold) is drawn as `hold.r = &t;` is (#531), t's borrow going into
// hold.r and returned from it at hold's last use, s staying borrowed
// until then; `*el = &t1` (el = &mut refs[0]) and `refs[0] = &t1` put
// t1's borrow into refs, as `refs.push(&t1)` does. They used to draw no
// borrow into hold.r / refs, and the only return at the end went to the
// old lender, as if the place still pointed there (#1361).
struct H<'a> { r: &'a String }
struct O<'a> { inner: H<'a>, n: i32 }
fn field() {
    let s = String::from("s");
    let t = String::from("t");
    let mut hold = H { r: &s };
    let rh = &mut hold;
    rh.r = &t;
    println!("{}", hold.r);
}
fn field_deref() {
    let s = String::from("s");
    let t = String::from("t");
    let mut hold = H { r: &s };
    let rh = &mut hold;
    (*rh).r = &t;
    println!("{}", hold.r);
}
fn field_read_through_ref() {
    let s = String::from("s");
    let t = String::from("t");
    let mut hold = H { r: &s };
    let rh = &mut hold;
    rh.r = &t;
    println!("{}", rh.r);
}
fn field_copy() {
    let s = String::from("s");
    let t = String::from("t");
    let tr = &t;
    let mut hold = H { r: &s };
    let rh = &mut hold;
    rh.r = tr;
    println!("{}", hold.r);
}
fn nested_field() {
    let a = String::from("a");
    let b = String::from("b");
    let mut o = O { inner: H { r: &a }, n: 1 };
    let ri = &mut o.inner;
    ri.r = &b;
    println!("{} {}", o.inner.r, o.n);
}
fn field_in_branch(c: bool) {
    let s = String::from("s");
    let t = String::from("t");
    let mut hold = H { r: &s };
    let rh = &mut hold;
    if c {
        rh.r = &t;
    }
    println!("{}", hold.r);
}
fn element() {
    let a1 = String::from("a1");
    let t1 = String::from("t1");
    let mut refs = vec![&a1];
    let el = &mut refs[0];
    *el = &t1;
    println!("{:?}", refs);
}
fn array_element() {
    let a1 = String::from("a1");
    let t1 = String::from("t1");
    let mut refs = [&a1];
    let el = &mut refs[0];
    *el = &t1;
    println!("{:?}", refs);
}
fn element_direct() {
    let a1 = String::from("a1");
    let t1 = String::from("t1");
    let mut refs = vec![&a1];
    refs[0] = &t1;
    println!("{:?}", refs);
}
fn element_in_branch(c: bool) {
    let a1 = String::from("a1");
    let t1 = String::from("t1");
    let mut refs = vec![&a1];
    if c {
        refs[0] = &t1;
    }
    println!("{:?}", refs);
}
fn element_mut() {
    let mut a1 = String::from("a1");
    let mut t1 = String::from("t1");
    let mut refs = vec![&mut a1];
    let el = &mut refs[0];
    *el = &mut t1;
    refs[0].push('x');
}
fn main() {
    field();
    field_deref();
    field_read_through_ref();
    field_copy();
    nested_field();
    field_in_branch(true);
    element();
    array_element();
    element_direct();
    element_in_branch(true);
    element_mut();
}
