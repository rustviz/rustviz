struct P { a: String, b: String }
fn main() {
    let t = (String::from("s"), P { a: String::from("a"), b: String::from("b") });
    match t {
        (s, P { a: x, .. } | P { b: x, .. }) => println!("{} {}", s, x),
    }
}
