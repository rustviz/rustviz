struct P {
    a: Option<String>,
    b: String,
}

fn make() -> String {
    String::from("m")
}

fn main() {
    let x = Some(String::from("x"));
    if let P { a: Some(s), .. } = (P { a: x, b: make() }) {
        println!("{}", s);
    }
    let y = Some(String::from("y"));
    match (P { a: y, b: make() }) {
        P { a: Some(t), .. } => println!("{}", t),
        _ => {}
    }
    let z = Some(String::from("z"));
    let P { a: Some(u), .. } = (P { a: z, b: make() }) else { return };
    println!("{}", u);
}
