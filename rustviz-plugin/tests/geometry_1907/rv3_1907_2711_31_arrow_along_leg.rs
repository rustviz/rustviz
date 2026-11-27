struct Pair {
    a: String,
    b: String,
}

fn make() -> String {
    String::from("made")
}

fn make_pair() -> Pair {
    Pair { a: make(), b: make() }
}

fn longest<'a>(a: &'a String, b: &'a String) -> &'a String {
    if a.len() > b.len() { a } else { b }
}

fn main() {
    let mut s = String::from("abc");
    let mut s2 = std::mem::replace(&mut s, String::new());
    let v = vec![String::from("x"), s2];
    Pair { a: s2, b: s } = make_pair();
    let r = &s2;
    if s2.len() > 1 {
        let r2 = longest(&s2, &s2);
    } else if r.is_empty() {
        println!("{} {:?}", s2, v);
    } else {
    }
}
