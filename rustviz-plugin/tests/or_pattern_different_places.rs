fn main() {
    let t = (String::from("a"), String::from("b"));
    match t {
        (x, _) | (_, x) => println!("{}", x),
    }
}
