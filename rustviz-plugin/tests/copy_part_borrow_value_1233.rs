// #1233: a reference to a Copy element or field of a value that needs
// drop points to a value (an `i32`, a `u32`, a `str`), not a resource:
// its dot says "borrows a value", as its own line already did. The lender
// keeps its "resource" wording (v is a Vec).
struct Pe { age: u32, name: String }
fn main() {
    let mut v = vec![1, 2];
    let e = &mut v[0];
    *e += 1;
    let f = &v[1];
    println!("{} {:?}", f, v);
    let mut p = Pe { age: 2, name: String::from("b") };
    let g = &mut p.age;
    *g += 1;
    let h = &p.name;
    println!("{}", h);
    let w = vec![String::from("x")];
    let x = &w[0];
    println!("{}", x);
    let s = String::from("hello");
    let sl = &s[..2];
    println!("{}", sl);
}
