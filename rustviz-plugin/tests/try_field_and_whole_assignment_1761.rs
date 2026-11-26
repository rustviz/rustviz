struct P { a: String, b: String }
fn pair() -> P { P { a: String::new(), b: String::new() } }
fn f6(x: Option<u8>, y: Option<u8>) -> Option<u8> {
    let mut p = pair();
    p.a = x?.to_string(); p = pair(); y?;
    println!("{}", p.a);
    Some(1)
}

fn moved_field(a: Option<u8>, b: Option<u8>) -> Option<()> {
    let mut pair = pair();
    let taken = pair.a;
    pair.a = a?.to_string(); b?;
    println!("{taken} {}", pair.a);
    Some(())
}
