fn consume(s: String) -> bool { s.len() > 3 }
fn main() {
    let mut s = String::from("hello");
    let opt = Some(5);
    if let Some(_) = opt && consume(s) {
        println!("long");
    } else {
        s = String::from("new");
        println!("{}", s);
    }
}
