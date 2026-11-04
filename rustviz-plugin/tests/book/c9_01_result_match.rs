fn main() {
    let r: Result<i32, String> = Ok(3);
    match r {
        Ok(n) => println!("{}", n),
        Err(e) => println!("error: {}", e),
    }
}
