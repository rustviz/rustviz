fn main() {
    let opt = Some(String::from("hi"));
    match opt {
        Some(s) => {
            println!("{}", s);
        }
        None => {}
    }
}
