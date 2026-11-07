struct P { a: String, b: String }
fn main() {
    let p = P { a: String::from("a"), b: String::from("b") };
    match p {
        P { a: x, .. } | P { b: x, .. } => println!("{}", x),
    }
}
