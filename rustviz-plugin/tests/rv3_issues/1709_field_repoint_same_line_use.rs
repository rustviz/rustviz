struct Holder<'a> {
    r: &'a String,
}
fn f(s: String, t: String, c: bool) {
    let mut h = Holder { r: &s };
    if c {
        h.r = &t; println!("{}", s); println!("{}", h.r);
    } else {
        println!("{}", h.r);
    }
}
