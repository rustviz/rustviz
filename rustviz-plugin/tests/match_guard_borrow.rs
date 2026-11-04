fn check(s: &String) -> bool { s.len() > 3 }
fn main() {
    let s = String::from("hello");
    let opt = Some(5);
    match opt {
        Some(n) if check(&s) => println!("{} {}", n, s),
        Some(n) => println!("{}", n),
        None => println!("none"),
    }
    println!("{}", s);
}
