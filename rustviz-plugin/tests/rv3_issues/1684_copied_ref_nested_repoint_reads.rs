fn f(s: String, t: String, a: bool, b: bool) {
    let x = &s;
    let mut r = x;
    if a {
        if b {
            r = &t;
        } else {
            println!("{}", r);
        }
    } else {
        println!("{}", r);
    }
}
