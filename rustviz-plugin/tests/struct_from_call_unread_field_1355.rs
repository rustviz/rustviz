// A struct (or tuple) a call returns, borrowing through a reference
// parameter: the loan in its type ends at its last use, whichever field
// that use reads, since it has no `Drop` impl: the unread field gives its
// borrow back there too, not at the end of the function (#1355).
struct Two<'p> { a: &'p str, b: &'p str }
fn two<'a>(c: &'a str, d: &'a str) -> Two<'a> { Two { a: c, b: d } }
fn pair<'a>(c: &'a str, d: &'a str) -> (&'a str, &'a str) { (c, d) }
fn otwo<'a>(c: &'a str) -> Option<Two<'a>> { Some(Two { a: c, b: c }) }
fn first<'a>(x: &'a str) -> &'a str { x }
fn kinds(s1: &String) {
    let d8 = two(s1, s1);
    println!("{}", d8.a);
    let r = first(s1);
    println!("{}", r);
}
fn tuple(s1: &String) {
    let d9 = pair(s1, s1);
    println!("{}", d9.0);
    println!("{}", s1);
}
fn unwrapped(s1: &String) {
    let e1 = otwo(s1).unwrap();
    println!("{}", e1.a);
    println!("{}", s1);
}
fn tried(s1: &String) -> Option<usize> {
    let e2 = otwo(s1)?;
    println!("{}", e2.b);
    Some(s1.len())
}
fn main() {
    let s = String::from("s");
    kinds(&s);
    tuple(&s);
    unwrapped(&s);
    tried(&s);
}
