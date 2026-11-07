fn main() {
    let a: Option<String> = None;
    let b = Some(String::from("b"));
    if let (Some(x), _) | (_, Some(x)) = (a, b) {
        println!("{}", x);
    } else {
        println!("none");
    }
}
