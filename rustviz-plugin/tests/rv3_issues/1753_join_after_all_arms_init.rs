fn f(s: String, t: String, u: String, a: bool, b: bool, c: bool) {
    let r;
    if a {
        r = &s;
    } else {
        r = &t;
    }
    println!("{}", r);
    let mut q = r;
    if b {
    } else {
        if c {
            q = &u;
            println!("{}", q);
        } else {
        }
    }
}
