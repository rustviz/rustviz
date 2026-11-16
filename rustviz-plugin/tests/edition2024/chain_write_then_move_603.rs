fn main() {
    let y = Some(String::from("new"));
    let mut b = Box::new(String::from("old"));
    if let Some(a) = y && { *b = a; let c = b; c.len() > 5 } {
        println!("long");
    } else {
        println!("short");
    }
}
