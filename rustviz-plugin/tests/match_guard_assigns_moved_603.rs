fn main() {
    let y = Some(1);
    let mut z = String::from("old");
    drop(z);
    match y {
        Some(_) if { z = String::from("new"); z.len() > 5 } => println!("{}", z),
        Some(_) => println!("some"),
        None => println!("none"),
    }
}
