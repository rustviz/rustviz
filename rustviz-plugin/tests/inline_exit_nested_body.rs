#![allow(irrefutable_let_patterns)]

fn outer(flag: bool, s: String) {
    fn inner(input: String) -> ! {
        if let moved = input {
            drop(moved);
            loop {}
        }
        panic!("imaginary path"); } drop(s);
    std::hint::black_box((flag, inner));
}
