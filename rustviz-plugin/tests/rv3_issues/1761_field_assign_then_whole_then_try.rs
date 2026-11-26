struct P { a: String, b: String }
fn pair() -> P { P { a: String::new(), b: String::new() } }
fn f6(x: Option<u8>, y: Option<u8>) -> Option<u8> {
    let mut p = pair();
    p.a = x?.to_string(); p = pair(); y?;
    println!("{}", p.a);
    Some(1)
}
