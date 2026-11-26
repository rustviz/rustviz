fn f(mut s: String, t: String, c: bool) {
    let mut r = &s;
    if c {
        s.push('x');
        r = &t;
    } else {
        println!("{}", r);
    }
}
