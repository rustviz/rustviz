fn main() {
    let b = Box::new(String::from("x"));
    drop(*b);
}
