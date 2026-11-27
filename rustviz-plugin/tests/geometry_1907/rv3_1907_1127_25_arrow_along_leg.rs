struct Holder<'a> {
    r: &'a String,
    n: i32,
}

fn longest<'a>(a: &'a String, b: &'a String) -> &'a String {
    if a.len() > b.len() { a } else { b }
}

fn holder<'a>(s: &'a String) -> Holder<'a> {
    Holder { r: s, n: 0 }
}

fn run(r: &String, mut v: Vec<String>) -> Option<usize> {
    let s = String::from("ab");
    if v.len() > 1 {
        let h = holder(r);
    } else if s.len() > 1 {
    } else {
        let mut r2 = longest(&s, &s);
    }
    Some(0)
}
