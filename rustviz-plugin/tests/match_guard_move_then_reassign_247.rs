fn consume(s: String) -> bool { s.len() > 3 }
fn main() {
    let mut s = String::from("hi");
    let opt = Some(5);
    match opt {
        Some(_) if consume(s) => println!("long"),
        _ => {
            s = String::from("new");
            println!("{}", s);
        }
    }
}
