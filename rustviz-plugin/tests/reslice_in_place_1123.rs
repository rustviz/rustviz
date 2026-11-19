// #1123: `slice = &mut slice[1..];` / `s = &s[1..];` give the reference
// a reborrow of part of what it already points to. The loan of v goes on
// until the reference's last use; v isn't given back at the reassignment.
fn mutable() {
    let mut v = vec![1, 2, 3];
    let mut slice: &mut [i32] = &mut v;
    slice = &mut slice[1..];
    slice[0] = 99;
    println!("{:?}", v);
}
fn shared() {
    let w = vec![1, 2, 3];
    let mut s: &[i32] = &w;
    s = &s[1..];
    let _spacer = 1;
    println!("{:?}", s);
}
fn in_loop(u: Vec<i32>) {
    let mut t: &[i32] = &u;
    while !t.is_empty() {
        t = &t[1..];
    }
    println!("{:?}", t);
}
fn whole() {
    let mut x = 5;
    let mut r = &mut x;
    r = &mut *r;
    *r += 1;
    println!("{}", x);
}
fn main() {
    mutable();
    shared();
    in_loop(vec![1]);
    whole();
}
