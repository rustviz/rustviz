fn run(o: Option<u8>) -> Option<u8> {
    let b = Box::new(String::from("a"));
    let s = *b;
    o?;
    Some(0)
}
