fn main() {
    let t = ((String::from("a"), String::from("b")), (String::from("c"), String::from("d")));
    match t {
        (x, _) | (_, x) => println!("{} {}", x.0, x.1),
    }
}
