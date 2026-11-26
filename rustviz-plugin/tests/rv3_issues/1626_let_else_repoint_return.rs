fn main() {
    let mut a = String::from("a");
    let mut b = String::from("b");
    let mut r = &mut a;
    let Some(_) = std::env::args().nth(1) else {
        r = &mut b;
        return;
    };
    *r = String::new();
}
