fn main() {
    let a = String::from("a");
    let b = String::from("b");
    let pair = (&a, &b);
    match pair {
        (x, y) => println!("{} {}", x, y),
    }
}
