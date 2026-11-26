fn main() {
    let mut b = Box::new(0);
    if std::hint::black_box(true) {
        *b = 1;
    } else {
        *b = 2;
    }
}
