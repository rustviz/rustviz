fn with_try(c: bool, d: bool, o: Option<u8>) -> Option<u8> {
    let mut z;
    if c {
        z = String::new();
    }
    if d {
        z = String::new();
    }
    o?;
    z = String::new();
    println!("{}", z);
    Some(0)
}
