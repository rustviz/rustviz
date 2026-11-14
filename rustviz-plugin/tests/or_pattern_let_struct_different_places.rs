struct P { n: String }
fn main() {
    let t = (P { n: String::from("a") }, P { n: String::from("b") });
    let ((x, _) | (_, x)) = t;
    println!("{}", x.n);
}
