fn main() {
    let s = String::from("hello");
    let t = String::from("hi");
    let y = Some(1);
    let mut r = &t;
    match y {
        Some(_) if { r = &s; r.len() > 3 } => println!("long"),
        _ => println!("other"),
    }
    println!("{}", r);
}
