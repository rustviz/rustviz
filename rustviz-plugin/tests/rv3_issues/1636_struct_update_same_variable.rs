struct Pair { a: String, b: String }
fn main() {
    let mut p = Pair { a: String::from("old"), b: String::new() };
    p = Pair { a: String::new(), ..p };
}
