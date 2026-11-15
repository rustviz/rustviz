struct P {
    name: String,
    tag: String,
}

fn make_p() -> P {
    P { name: String::from("n"), tag: String::from("t") }
}

fn main() {
    let x = Some(String::from("x"));
    let y = make_p();
    if let (Some(a), _) = (x, y.name) {
        println!("{}", a);
    } else {
        println!("none");
    }
    let w = Some(String::from("w"));
    let p = (String::from("p0"), String::from("p1"));
    match (w, p.0) {
        (Some(e), _) => println!("{}", e),
        _ => println!("none"),
    }
    let u = Some(String::from("u"));
    let q = make_p();
    if let (Some(c), n) = (u, q.name) {
        println!("{} {}", c, n);
    }
    let v = Some(String::from("v"));
    let b = Box::new(make_p());
    if let (Some(d), _) = (v, b.name) {
        println!("{}", d);
    }
}
