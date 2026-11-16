fn consume(s: String) -> bool { s.len() > 3 }
fn main() {
    let s = String::from("hello");
    let opt = Some(5);
    match opt {
        Some(_) if consume(s) => println!("long"),
        Some(_) => println!("short"),
        None => println!("none"),
    }
}
