fn run(c: bool, d: bool, e: bool, o: Option<u8>) -> Option<u8> {
    let mut s;
    if c {
        s = String::new();
    }
    if d {
        if e {
            s = String::new();
            return None;
        }
    }
    o?;
    Some(0)
}
