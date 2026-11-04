fn main() {
    let maybe: Option<String> = Some(String::from("x"));
    let s = maybe.expect("should have a value");
    println!("{}", s);
}
