fn main() {
    let a: Option<String> = Some(String::from("a"));
    match a {
        Some(x) => println!("{}", x),
        None => {}
    }
    let b: Option<String> = Some(String::from("b"));
    match b {
        Some(x) => println!("{}", x),
        None => {}
    }
}
