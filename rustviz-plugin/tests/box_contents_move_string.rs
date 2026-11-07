fn main() {
    let b = Box::new(String::from("a"));
    let s = *b;
    println!("{}", s);
}
