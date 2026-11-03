fn main() {
    let name: Option<String> = Some(String::from("Ann"));
    if let Some(n) = name {
        println!("hello {}", n);
    } else {
        println!("nobody");
    }
}
