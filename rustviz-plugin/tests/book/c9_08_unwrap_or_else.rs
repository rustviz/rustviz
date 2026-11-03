fn main() {
    let r: Result<i32, String> = Err(String::from("bad"));
    let n = r.unwrap_or_else(|e| e.len() as i32);
    println!("{}", n);
}
