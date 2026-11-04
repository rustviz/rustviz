fn main() {
    let x = Some(String::from("a"));
    let y = Some(String::from("b"));
    match (x, y) {
        (Some(a), Some(b)) => println!("{} {}", a, b),
        _ => println!("none"),
    }

    let t = (String::from("a"), (1, String::from("b")));
    match t {
        (first, (n, second)) if n > 0 => println!("{} {}", first, second),
        _ => {}
    }
}
