fn f5(c: bool) {
    let b = Box::new(String::from("a"));
    if c {
        let _s = *b;
        return;
    }
    drop(b);
}
