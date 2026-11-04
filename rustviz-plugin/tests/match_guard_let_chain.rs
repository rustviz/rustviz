fn check(s: &String) -> bool { s.len() > 3 }
fn main() {
    let s = String::from("hello");
    let words = vec![String::from("a"), String::from("bb")];
    match words.len() {
        0 => println!("empty"),
        n if let Some(w) = words.last() && check(&s) => {
            println!("{} {} {}", n, w, s);
        }
        _ => println!("other"),
    }
}
