fn main() {
    let mut t = (String::from("a"), String::from("b"));
    match t {
        (ref mut x, _) => x.push_str("?"),
    }
    let r: Result<String, String> = Ok(String::from("c"));
    match r {
        Ok(x) => println!("{}", x),
        Err(_) => {}
    }
}
