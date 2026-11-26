fn run(s: String, t: String, c: bool) {
    let x = &s;
    let mut r = x;
    if c {
        r = &t;
    } else {
        println!("{}", r);
    }
}
