fn main() {
    let a: Option<String> = None;
    let b = Some(String::from("b"));
    match (a, b) {
        (Some(x), _) | (_, Some(x)) => println!("{}", x),
        _ => println!("none"),
    }
}
