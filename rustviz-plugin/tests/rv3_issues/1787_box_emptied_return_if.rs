fn returns_other(c: bool, early: bool, q: Box<String>) -> Box<String> {
    let mut b = Box::new(String::from("a"));
    let _old = *b;
    if early {
        return if c { b = Box::new(String::new()); b } else { q };
    }
    q
}
