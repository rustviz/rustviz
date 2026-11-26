fn make() -> Option<String> { Some(String::new()) }
fn f(ready: bool, k: u8) {
    match k {
        _ if let Some(_) = make() && ready => println!("a"),
        _ => println!("b"),
    }
}
