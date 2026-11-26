fn f(c: bool, q: Box<String>) -> Box<String> {
    let mut b = Box::new(String::from("a"));
    let _old = *b;
    if true {
        return if c { b = Box::new(String::new()); b } else { q };
    }
    q
}
