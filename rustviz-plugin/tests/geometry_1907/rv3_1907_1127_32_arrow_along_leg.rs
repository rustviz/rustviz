struct Holder<'a> {
    r: &'a String,
    n: i32,
}

fn downgrade(s: &mut String) -> &String {
    s
}

fn holder<'a>(s: &'a String) -> Holder<'a> {
    Holder { r: s, n: 0 }
}

fn main() {
    let mut s = String::from("abc");
    if s.len() > 1 {
        let h = holder(&s);
    } else if s.len() > 1 {
        let r = downgrade(&mut s);
    } else {
    }
}
