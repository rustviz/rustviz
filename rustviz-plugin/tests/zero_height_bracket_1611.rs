// #1611: r is bound and ended on one dot; its bracket was
// drawn with no height, a 15 px stub off the dot.
fn f(s: &mut String) {
    let a = String::from("a");
    let x = &a;
    let r = x;
    let m = &mut *s;
    let q = &*m;
    let n = 1;
    println!("{}", n);
}
fn main() { let mut s = String::new(); f(&mut s); }
