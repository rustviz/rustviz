fn f(o: Option<u8>) -> Option<u8> {
    let c = &String::from_utf8_lossy(b"hi");
    o?;
    println!("{c}");
    Some(0)
}
