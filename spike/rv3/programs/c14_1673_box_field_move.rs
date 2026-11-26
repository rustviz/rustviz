struct P { a: String, b: String }
fn main() {
    let b = Box::new(P { a: String::from("a"), b: String::from("b") });
    let x = b.a;
    println!("{} {}", x, b.b);
}
