// Method-call auto-referencing (autoref): `s.len()` is `String::len(&s)`
// and `s.push_str(..)` is `String::push_str(&mut s, ..)`. rustc inserts
// the `&` / `&mut`, so `s` is borrowed for the call, not moved, and the
// call dot says "(implicitly borrowed)". When the `&` is written
// (`(&s).is_empty()`, `String::len(&s)`), there's no note.
fn main() {
    let mut s = String::from("hi");
    let n = s.len();
    s.push_str(" there");
    let e = (&s).is_empty();
    let k = String::len(&s);
    String::push_str(&mut s, "?");
    let mut v = vec![1, 2, 3];
    v.push(4);
    let b = Box::new(String::from("boxed"));
    let bl = b.len();
    println!("{} {} {} {} {} {:?}", n, e, k, bl, s, v);
}
