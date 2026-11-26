fn run(c: bool, d: bool) {
    let a = String::from("a");
    let b = String::from("b");
    let q = &b;
    let mut r = &a;
    if c {
        r = &b;
    } else {
        r = &b;
    }
    if d { return; }
    r.len();
    q.len();
}
