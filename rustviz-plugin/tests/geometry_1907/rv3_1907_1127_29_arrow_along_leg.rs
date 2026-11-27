struct Pair {
    a: String,
    b: String,
}

fn main() {
    let s = String::from("hey");
    let p = Pair { a: format!("{}!", s), b: String::from("abc") };
    let s2 = s;
    let Pair { b: r, .. } = &p;
    if r.is_empty() {
        let r2 = &p.b;
    } else if s2.len() > 1 {
    } else {
        if r.is_empty() { return; }
    }
}
