fn make() -> String {
    String::from("m")
}

fn main() {
    let x = Some(String::from("x"));
    if let (Some(a), _) = (x, make()) {
        println!("{}", a);
    } else {
        println!("none");
    }
    let y = Some(String::from("y"));
    match (y, make()) {
        (Some(b), _) => println!("{}", b),
        _ => println!("none"),
    }
    let z = Some(String::from("z"));
    if let (Some(c), m) = (z, make()) {
        println!("{} {}", c, m);
    }
    let w = Some(String::from("w"));
    match (w, make()) {
        (Some(d), _) => println!("{}", d),
        (None, n) => println!("{}", n),
    }
}
