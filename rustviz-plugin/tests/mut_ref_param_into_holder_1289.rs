// #1289: a `&mut` parameter moved (no reborrow: a generic `T` field or
// argument, an array element) into a value that keeps its borrow. The
// value's holder k now holds the caller's borrow, until k's last use;
// r is moved and has no borrow of its own to end. main ended r's borrow
// at the move ("r's mutable borrow ends") and drew k holding no borrow,
// or, for `vec![r]`, lent r to k after moving it (`State::Invalid`).
fn id<T>(x: T) -> T { x }
fn p_some(r: &mut String) {
    let k = Some(r);
    if let Some(x) = k { x.push('!'); }
}
fn p_vec(r: &mut String) {
    let k = vec![r];
    println!("{}", k.len());
}
// `k.push('!')` writes through the `&mut` in k's Box (#1282).
fn p_box(r: &mut String) {
    let k = Box::new(r);
    k.push('!');
}
fn p_id_some(r: &mut String) {
    let k = id(Some(r));
    println!("{}", k.is_some());
}
fn p_box_unused(r: &mut String) {
    let k = Box::new(r);
    let n = 1;
    println!("{}", n);
}
fn main() {
    let mut s = String::new();
    p_some(&mut s);
    p_vec(&mut s);
    p_box(&mut s);
    p_id_some(&mut s);
    p_box_unused(&mut s);
}
