fn main() {
    let o: Option<Result<String, String>> = Some(Ok(String::from("x")));
    match o {
        Some(Ok(s) | Err(s)) => println!("{}", s),
        None => println!("none"),
    }
    let r: Result<String, String> = Err(String::from("e"));
    match &r {
        Ok(t) | Err(t) => println!("{}", t),
    }
    println!("{:?}", r);
    let n: Result<i32, i32> = Ok(3);
    match n {
        Ok(k) | Err(k) if k > 0 => println!("{}", k),
        _ => println!("other"),
    }
}
