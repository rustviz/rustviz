fn main() {
    let mut o: Option<String> = Some(String::from("a"));
    let n = 3;
    match o {
        Some(ref mut s) if n > 2 => {
            s.push_str("!");
            println!("{:?}", o);
        }
        _ => {}
    }
}
