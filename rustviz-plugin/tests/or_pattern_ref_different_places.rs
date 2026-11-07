fn main() {
    let t = (String::from("a"), String::from("b"));
    match t {
        (ref x, _) | (_, ref x) => println!("{}", x),
    }
}
