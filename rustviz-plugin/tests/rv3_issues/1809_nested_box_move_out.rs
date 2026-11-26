fn main() {
    let boxed = Box::new(Box::new(String::from("hello")));
    let text = **boxed;
    drop(text);
}
