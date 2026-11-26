fn f(mut s: String, t: String, u: String, b: bool) {
    let mut r = &s;
    if r.is_empty() {
        s.push('y');
    } else {
        s.push('z');
    }
    if b {
        r = &t;
    } else {
        r = &u;
    }
    println!("{}", r);
}
