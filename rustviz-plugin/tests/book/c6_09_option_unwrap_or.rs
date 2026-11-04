fn main() {
    let x: Option<i32> = Some(2);
    let y = x.unwrap_or(0);
    let b = x.is_some();
    println!("{} {}", y, b);
}
