// #1256: an `if` / `match` written as a tuple element or a struct-literal
// field gives its part of the value from each branch, in that branch: d
// moves into the literal on its path only, e on the other. The parts
// that aren't conditional (`b: f`) are moved after it, on the same line.
struct W { s: String }
struct P { a: String, b: String }
fn tup(c: bool) {
    let d = String::from("d");
    let e = String::from("e");
    let _t = (if c { d } else { e }, 1);
}
fn lit(c: bool) {
    let d = String::from("d");
    let e = String::from("e");
    let _w = W { s: if c { d } else { e } };
}
fn mat(n: i32) {
    let d = String::from("d");
    let e = String::from("e");
    let f = String::from("f");
    let _p = P { a: match n { 0 => d, _ => e }, b: f };
}
fn destr(c: bool) {
    let d = String::from("d");
    let e = String::from("e");
    let (x, _n) = (if c { d } else { e }, 1);
    println!("{}", x);
}
fn local_in_branch(c: bool) {
    let d = String::from("d");
    let _t = (if c { let z = String::from("z"); z } else { d }, 1);
}
fn mat_f(n: i32) {
    let d = String::from("d");
    let e = String::from("e");
    let f = String::from("f");
    let _q = (match n { 0 => d, _ => e }, f);
}
fn destr_f(n: i32) {
    let d = String::from("d");
    let e = String::from("e");
    let f = String::from("f");
    let (y, _g) = (match n { 0 => d, _ => e }, f);
    println!("{}", y);
}
