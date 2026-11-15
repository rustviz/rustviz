fn main() {
    let s = String::from("hello");
    let t = String::from("hi");
    let y = Some(1);
    let mut r = &t;
    if let Some(_n) = y && { r = &s; r.len() > 3 } {
        println!("long");
    } else {
        println!("short");
    }
    println!("{}", r);
}
