fn run(c: bool, d: bool, n: usize) {
    let mut s = String::from("a");
    drop(s);
    for i in 0..n {
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
