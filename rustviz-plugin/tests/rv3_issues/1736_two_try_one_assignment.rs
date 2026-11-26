fn make(_a: u8, _b: u8) -> String { String::new() }
fn f4(o: Option<u8>, p: Option<u8>) -> Option<u8> {
    let mut s = String::from("a");
    println!("{}", s);
    s = make(o?, p?);
    println!("{}", s);
    Some(1)
}
