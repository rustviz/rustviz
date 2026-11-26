fn f(s: String, t: String, a: bool, b: bool) {
    let r;
    if a {
        r = &s;
        println!("{}", r);
    } else {
        if b {
            r = &t;
            println!("{}", r);
        } else {
        }
    }
}
