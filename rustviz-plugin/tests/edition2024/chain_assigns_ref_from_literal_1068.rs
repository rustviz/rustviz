fn main() {
    let s = String::from("hello");
    let y = Some(1);
    let mut r: &str = "hi";
    if let Some(_n) = y && { r = &s; r.len() > 3 } {
        println!("long {}", r);
    } else {
        println!("short {}", r);
    }
}
