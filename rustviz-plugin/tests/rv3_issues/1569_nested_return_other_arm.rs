fn run(c: bool) {
    let s = String::from("abc");
    let vr = vec![&s];
    if c {
        drop(s);
        if c { return; }
    } else {
        println!("{:?}", vr);
    }
}
