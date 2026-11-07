struct P { a: String, b: String }
fn main() {
    let mut p = P { a: String::from("a"), b: String::from("b") };
    let (P { a: x, .. } | P { b: x, .. }) = p;
    println!("{}", x);
    p.a = String::from("c");
    println!("{}", p.a);
}
