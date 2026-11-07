fn main() {
    let o: Option<String> = Some(String::from("a"));
    let n = 3;
    match o {
        Some(ref s) if n > 2 => {
            println!("{}", s);
        }
        _ => {}
    }
}
