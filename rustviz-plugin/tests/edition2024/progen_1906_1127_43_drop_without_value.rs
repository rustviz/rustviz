struct Pair {
    a: String,
    b: String,
}

fn make() -> String {
    String::from("made")
}

fn wrap_mut<'a>(s: &'a mut String) -> &'a mut String {
    s
}

fn main() {
    let mut s = String::from("hi");
    if s.len() > 1 { return; }
    let mut p2 = Pair { a: String::from("abc"), b: String::from("ab") };
    let Pair { a: s2, b: s3 } = p2;
    p2 = Pair { a: make(), b: String::from("x") };
    let m = wrap_mut(&mut s);
    if m.len() > 2 { println!("{} {}", s2, s3); }
}
