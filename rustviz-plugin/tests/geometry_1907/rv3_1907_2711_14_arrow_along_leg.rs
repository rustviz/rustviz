struct Holder<'a> {
    r: &'a String,
    n: i32,
}

fn holder<'a>(s: &'a String) -> Holder<'a> {
    Holder { r: s, n: 0 }
}

fn main() {
    let s = String::from("ab");
    let s2 = s;
    let vr = vec![s2.as_str(), "rust"];
    let h = holder(&s2);
    if s2.len() > 1 {
        let r = &s2;
    } else if vr.is_empty() {
        drop(h);
    }
}
