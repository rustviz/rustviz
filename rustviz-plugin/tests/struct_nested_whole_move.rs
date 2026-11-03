// #174: moving `r` moves the nested `r.a` and `r.a.b` with it.
struct A { b: String }
struct R { a: A }
fn main() {
    let r = R { a: A { b: String::from("hi") } };
    let s = r;
    println!("{}", s.a.b);
}
