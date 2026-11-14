struct P { n: String }
fn main() {
    let t = (Some(P { n: String::from("a") }), Some(P { n: String::from("b") }));
    if let (Some(x), _) | (_, Some(x)) = t {
        println!("{}", x.n);
    }
}
