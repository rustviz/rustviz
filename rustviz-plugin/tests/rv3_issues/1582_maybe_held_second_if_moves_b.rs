fn run(c: bool, d: bool, o: Option<u8>) -> Option<u8> {
    let mut s = String::from("a");
    drop(s);
    if c {
        s = String::from("b");
    }
    if d {
    } else {
        let n = o?;
    }
    Some(0)
}
