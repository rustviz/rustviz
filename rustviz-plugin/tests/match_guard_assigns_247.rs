fn main() {
    let mut z = String::from("old");
    let opt = Some(5);
    match opt {
        Some(n) if { z = n.to_string(); n > 3 } => println!("big {}", z),
        _ => println!("other {}", z),
    }
}
