fn main() {
    let r: Result<String, String> = Ok(String::from("x"));
    if let Ok(s) | Err(s) = r {
        println!("{}", s);
    }
}
