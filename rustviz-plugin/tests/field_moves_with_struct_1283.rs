// #1283: a struct moved as a whole moves its fields on the same row. With
// a second arrow on the line (`r`'s borrow ending at `*r = t;`), main drew
// the field's move a row below the struct's, so the field was still drawn
// owned after the struct had moved.
struct P { name: String }
struct O { inner: P }
fn into_deref() {
    let t = P { name: String::from("t") };
    let mut p = P { name: String::new() };
    let r = &mut p;
    *r = t;
    println!("{}", p.name);
}
fn into_deref_in_arm(o: Option<i32>) {
    let t = P { name: String::from("t") };
    let mut p = P { name: String::new() };
    let r = &mut p;
    match o {
        Some(_) => *r = t,
        None => {}
    }
    println!("{}", p.name);
}
fn into_field_through_ref() {
    let t = P { name: String::from("t") };
    let mut o = O { inner: P { name: String::new() } };
    let r = &mut o;
    r.inner = t;
    println!("{}", o.inner.name);
}
fn nested_into_deref() {
    let t = O { inner: P { name: String::from("t") } };
    let mut o = O { inner: P { name: String::new() } };
    let r = &mut o;
    *r = t;
    println!("{}", o.inner.name);
}
fn into_let_literal() {
    let q = P { name: String::from("q") };
    let (n, ref r) = (1, q);
    println!("{} {}", n, r.name);
}
fn main() {
    into_deref();
    into_deref_in_arm(None);
    into_field_through_ref();
    nested_into_deref();
    into_let_literal();
}
