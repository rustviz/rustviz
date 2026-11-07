fn main() {
    let mut o: Option<String> = Some(String::from("a"));
    match o {
        Some(ref s) => {
            println!("{}", s);
            o.take();
        }
        None => {}
    }
}
