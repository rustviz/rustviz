fn take(b: Box<String>) -> usize { b.len() }
fn main() {
    let b = Box::new(String::from("a"));
    take(b);
}
