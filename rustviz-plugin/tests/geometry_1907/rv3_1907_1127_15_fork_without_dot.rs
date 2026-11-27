fn main() {
    let s = String::from("ab");
    let r = &s;
    if r.is_empty() {
    } else {
        let s2;
        if s.len() > 1 { } else { s2 = r.to_string(); }
    }
}
