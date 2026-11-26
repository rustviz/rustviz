fn run(c: bool, d: bool) {
    let mut s = String::from("a");
    drop(s);
    for i in 0..2 {
        s = String::from("b");
    }
    if c {
        s = String::from("b");
    }
    if d {
        s = String::from("c");
    } else {
    }
}
