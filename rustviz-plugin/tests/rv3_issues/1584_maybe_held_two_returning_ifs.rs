fn run(c: bool, d: bool, e: bool, f: bool) {
    let mut s = String::from("ab");
    drop(s);
    if c {
        s = String::from("ab");
    }
    if d {
        if e {
            return;
        }
        if f {
            let t = String::from("x");
            return;
        }
    } else {
    }
}
