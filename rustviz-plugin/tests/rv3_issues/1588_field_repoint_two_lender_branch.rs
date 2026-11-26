struct Holder<'a> {
    r: &'a String,
}

fn longest<'a>(a: &'a String, b: &'a String) -> &'a String {
    if a.len() > b.len() { a } else { b }
}

fn run(s: String, t: String, c: bool) {
    let mut h = Holder { r: longest(&s, &t) };
    if c {
        h.r = &t;
    } else {
    }
}
