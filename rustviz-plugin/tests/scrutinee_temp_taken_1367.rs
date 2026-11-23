fn make() -> String {
    String::from("m")
}

fn make_opt() -> Option<String> {
    Some(String::from("o"))
}

struct P {
    name: String,
    tag: String,
}

fn make_p() -> P {
    P { name: String::from("n"), tag: String::from("t") }
}

fn if_let_tuple(z: Option<String>) {
    if let (Some(c), m) = (z, make()) {
        println!("{} {}", c, m);
    }
}

fn match_tuple(z: Option<String>) {
    match (z, make()) {
        (Some(c), m) => println!("{} {}", c, m),
        _ => {}
    }
}

fn ctor_guard() {
    match Some(make()) {
        Some(r) if r.len() > 3 => println!("{}", r),
        _ => {}
    }
}

fn call_guard() {
    match make_opt() {
        Some(r) if r.len() > 3 => println!("{}", r),
        _ => {}
    }
}

fn ok_or_err() {
    match Ok::<String, String>(make()) {
        Ok(r) if r.len() > 2 => println!("{}", r),
        Err(e) => println!("{}", e),
        _ => {}
    }
}

fn part_left(z: Option<String>) {
    if let (Some(c), P { name, .. }) = (z, make_p()) {
        println!("{} {}", c, name);
    }
}

fn main() {
    if_let_tuple(Some(String::from("z")));
    match_tuple(None);
    ctor_guard();
    call_guard();
    ok_or_err();
    part_left(None);
}
