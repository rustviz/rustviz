struct Q {
    name: String,
    tag: String,
}

fn make_q() -> Q {
    Q { name: String::from("n"), tag: String::from("t") }
}

fn main() {
    let z = Some(String::from("z"));
    if let (Some(d), n) = (z, make_q().name) {
        println!("{} {}", d, n);
    }
}
