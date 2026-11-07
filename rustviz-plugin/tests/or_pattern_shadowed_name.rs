fn main() {
    let t = (String::from("a"), String::from("b"));
    match t {
        (x, _) | (_, x) => println!("{}", x),
    }
    let u = (String::from("c"), String::from("d"));
    match u {
        (x, _) | (_, x) => println!("{}", x),
    }
    let mut w = (String::from("e"), String::from("f"));
    match w {
        (ref mut x, _) | (_, ref mut x) => x.push_str("!"),
    }
}
