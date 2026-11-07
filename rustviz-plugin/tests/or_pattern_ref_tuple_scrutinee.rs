fn main() {
    let a: Option<String> = None;
    let b = Some(String::from("b"));
    match (a, b) {
        (Some(ref x), _) | (_, Some(ref x)) => println!("{}", x),
        _ => println!("none"),
    }
}
