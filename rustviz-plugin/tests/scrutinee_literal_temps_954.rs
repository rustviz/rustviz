struct P {
    a: Option<String>,
    b: String,
}

struct Q {
    name: String,
    tag: String,
}

fn make() -> String {
    String::from("m")
}

fn make_q() -> Q {
    Q { name: String::from("n"), tag: String::from("t") }
}

fn main() {
    let x = Some(String::from("x"));
    if let P { a: Some(s), .. } = (P { a: x, b: make() }) {
        println!("{}", s);
    } else {
        println!("none");
    }
    let y = Some(String::from("y"));
    if let (Some(c), _) = (y, make_q().name) {
        println!("{}", c);
    } else {
        println!("none");
    }
    let w = Some(String::from("w"));
    match (w, make_q().name) {
        (Some(e), _) => println!("{}", e),
        _ => println!("none"),
    }
    let v = Some(String::from("v"));
    let (Some(a), _) = (v, make()) else {
        return;
    };
    println!("{}", a);
}
