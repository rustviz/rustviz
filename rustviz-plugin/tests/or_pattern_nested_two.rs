fn main() {
    let t = ((String::from("a"), String::from("b")), (1, 2));
    match t {
        ((x, _) | (_, x), (n, _) | (_, n)) => println!("{} {}", x, n),
    }
}
