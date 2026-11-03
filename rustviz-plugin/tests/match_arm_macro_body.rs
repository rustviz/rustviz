fn main() {
    let opt = Some(String::from("hi"));
    match opt {
        Some(s) => println!("{}", s),
        None => println!("none"),
    }
    let res: Result<i32, String> = Ok(3);
    match res {
        Ok(n) => println!("{}", n),
        Err(e) => println!("{}", e),
    }
}
