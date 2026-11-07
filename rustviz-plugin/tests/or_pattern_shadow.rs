fn main() {
    let s = String::from("outer");
    let r: Result<String, String> = Ok(String::from("x"));
    match r {
        Ok(s) | Err(s) => println!("{}", s),
    }
    println!("{}", s);
}
