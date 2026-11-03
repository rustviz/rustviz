fn main() {
    let maybe: Option<String> = Some(String::from("x"));
    let s = maybe.expect("should have a value");
    let r: Result<i32, String> = Err(String::from("bad"));
    let n = r.unwrap_or_else(|e| e.len() as i32);
    let c: Option<i32> = Some(2);
    let k = c.unwrap_or(0);
    let t = s.clone();
    println!("{} {} {} {} {}", s, n, c.is_some(), k, t);
}
