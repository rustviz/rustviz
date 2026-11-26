fn main() {
    let mut b = Box::new(String::from("a"));
    if std::hint::black_box(true) {
        *b = String::from("b");
    } else {
        *b = String::from("c");
    }
}
