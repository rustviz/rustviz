fn main() {
    let t = (String::from("a"), (String::from("b"), String::from("c")));
    match t {
        (ref a, (ref x, _) | (_, ref x)) => println!("{} {}", a, x),
    }
}
