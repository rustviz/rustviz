fn f(s: String, t: String, u: String, c: bool) {
    let mut r = &s;
    if c {
        r = &t;
    } else {
        println!("{}", r);
    }
    r = &u;
    println!("{}", r);
}
