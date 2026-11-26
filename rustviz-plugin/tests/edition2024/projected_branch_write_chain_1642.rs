fn main() {
    let mut chain = Box::new(0);
    if let Some(value) = std::hint::black_box(Some(1)) && value > 0 {
        *chain = value;
    } else {
        *chain = 2;
    }
    assert_eq!(*chain, 1);
}
