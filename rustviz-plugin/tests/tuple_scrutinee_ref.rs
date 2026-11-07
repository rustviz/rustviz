fn main() {
    let x = Some(String::from("a"));
    let y = Some(String::from("b"));
    match (x, y) {
        (Some(ref a), _) => println!("{}", a),
        _ => println!("none"),
    }

    let p = Some(String::from("p"));
    let q = Some(String::from("q"));
    if let (Some(ref mut s), Some(t)) = (p, q) {
        s.push_str(&t);
        println!("{}", s);
    }
}
