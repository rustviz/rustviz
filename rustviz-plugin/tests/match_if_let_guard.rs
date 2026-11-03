fn describe(s: &String) -> usize { s.len() }
fn main() {
    let s = String::from("hello");
    let opt: Option<i32> = Some(3);
    match opt {
        Some(n) if let Some(m) = n.checked_mul(2) => println!("{} {}", m, describe(&s)),
        _ => println!("none"),
    }
}
