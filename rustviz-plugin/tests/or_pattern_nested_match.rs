fn main() {
    let t = (String::from("a"), (String::from("b"), String::from("c")));
    match t {
        (a, (x, _) | (_, x)) => println!("{} {}", a, x),
    }
}
