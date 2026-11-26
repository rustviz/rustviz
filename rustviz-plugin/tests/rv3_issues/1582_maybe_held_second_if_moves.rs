fn run(c: bool, d: bool) {
    let mut s = String::from("a");
    drop(s);
    if c {
        s = String::from("b");
    }
    if d {
        s = String::from("c");
        drop(s);
    } else {
    }
}
