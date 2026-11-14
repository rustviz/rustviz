struct P { n: String }
fn main() {
    let t = (Some(P { n: String::from("a") }), Some(P { n: String::from("b") }));
    let ((Some(x), _) | (_, Some(x))) = t else { return };
    println!("{}", x.n);
}
