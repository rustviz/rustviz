fn take(s: String) -> bool {
    s.len() > 5
}

fn main() {
    let y = Some(1);
    let mut z = String::from("old");
    match y {
        Some(_) if { z = String::from("new"); take(z) } => println!("taken"),
        _ => println!("other"),
    }
}
