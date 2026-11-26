struct Holder<'a> { r: &'a String }
fn longest<'a>(a: &'a String, b: &'a String) -> &'a String { if a.len() > b.len() { a } else { b } }
fn run(s: String, t: String, u: String) {
    let mut h = Holder { r: longest(&s, &t) };
    let n = 1;
    h.r = &t; h.r = &u;
}
