struct Pair {
    a: String,
    b: String,
}

fn main() {
    let s = String::from("hey");
    let s2 = String::from("ab");
    let mut b = Box::new(s.clone());
    let p = Pair { a: (*b).clone(), b: s2.to_string() };
    let mut r = &*b;
    if b.len() > 2 {
        if r.is_empty() {
        } else if r.is_empty() {
        } else {
            if s.len() > 1 { return; }
        }
    } else {
        let m2 = &mut *b;
    }
    let s5;
    s5 = String::from("hi");
}
