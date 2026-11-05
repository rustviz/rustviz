// A reference to a Copy value (`&mut i32`, `&i32`) says "the borrowed
// value" / "the value y points to", like the i32 column it borrows
// from ("x holds a value"; #312). References to a String keep
// "resource". The wording follows the reference's type, so it holds
// for parameters too. A write `*s = ...` is an assignment and reads like
// a plain one (2026-11-05 decision: "*s acquires ownership of a new
// resource; its previous resource is dropped"); the in-place update
// `*n += *k` keeps "modified through".

fn bump(n: &mut i32, k: &i32, s: &mut String) {
    *n += *k;
    *s = String::from("new");
}

fn main() {
    let mut x = 7;
    let y = &mut x;
    *y += 3;
    let z = &x;
    println!("{}", z);
    let mut s = String::from("hi");
    let b = 2;
    bump(&mut x, &b, &mut s);
    let q = &mut s;
    q.push('!');
    println!("{} {}", x, s);
}
