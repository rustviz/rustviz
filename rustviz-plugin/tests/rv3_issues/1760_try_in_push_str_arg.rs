fn f2(a: Option<&str>) -> Option<u8> {
    let mut s = String::new();
    s.push_str(a?);
    println!("{}", s);
    Some(1)
}
