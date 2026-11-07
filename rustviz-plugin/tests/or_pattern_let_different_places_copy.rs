fn main() {
    let t = (1, 2);
    let ((x, _) | (_, x)) = t;
    if let (y, 5) | (5, y) = t {
        println!("{} {}", x, y);
    }
}
