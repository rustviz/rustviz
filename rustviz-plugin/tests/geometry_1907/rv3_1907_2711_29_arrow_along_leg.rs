fn main() {
    let s = String::from("x");
    let mut s2 = s;
    let vr = vec![s2.as_str(), "ab"];
    let r = &s2;
    if s2.len() > 1 {
    } else if r.is_empty() {
        if s2.len() > 1 { return; }
    } else {
        let m3 = &mut s2;
    }
}
