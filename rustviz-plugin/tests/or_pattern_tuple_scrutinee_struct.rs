struct P { n: String }
fn main() {
    let a = P { n: String::from("a") };
    let b = P { n: String::from("b") };
    match (a, b) {
        (x, _) | (_, x) => println!("{}", x.n),
    }
}
