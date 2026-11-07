fn main() {
    let t = (String::from("a"), String::from("b"), String::from("c"));
    match t {
        (x, _, _) | (_, x, _) | (_, _, x) => println!("{}", x),
    }
}
