struct Pair {
    a: String,
    b: String,
}

fn main() {
    let s = String::from("rust");
    let mut p = Pair { a: String::from("hey"), b: String::from("ab") };
    if s.len() > 1 {
    } else if s.len() > 1 {
        p = Pair { a: s, b: String::from("abc") };
        let s2 = p.a;
    }
}
