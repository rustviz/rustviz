struct Pair { a: String, b: String }
fn f(mut x: String, mut y: String, r: Pair) {
    if x.len() > 0 {
        Pair { a: x, b: y } = r;
    }
}
