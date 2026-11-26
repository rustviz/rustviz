fn run(c: bool) {
    let s = String::new();
    let s2 = String::new();
    let mut r = &s;
    if c {
        println!("{}", r);
    } else {
        if r.is_empty() {
            r = &s2;
        } else {
            println!("x");
        }
    }
}
