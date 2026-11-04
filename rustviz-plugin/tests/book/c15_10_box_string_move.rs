fn main() {
    let b = Box::new(String::from("hi"));
    let c = b;
    println!("{}", c);
}
