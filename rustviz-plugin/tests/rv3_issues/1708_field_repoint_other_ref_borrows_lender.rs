struct Holder<'a> {
    r: &'a String,
}
fn longest<'a>(a: &'a String, b: &'a String) -> &'a String {
    if a.len() > b.len() { a } else { b }
}
fn f(s: String, t: String, u: String, c: bool) {
    let other = &s;
    let mut h = Holder { r: longest(&s, &t) };
    if c {
        h.r = &u;
    } else {
        println!("{}", h.r);
    }
    println!("{}", other);
}
