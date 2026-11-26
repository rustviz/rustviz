fn run(c: bool, d: bool) {
    let mut b = Box::new(String::from("ab"));
    if c {
        let s = *b;
    }
    if d {
        b = Box::new(String::from("cd"));
    }
}
