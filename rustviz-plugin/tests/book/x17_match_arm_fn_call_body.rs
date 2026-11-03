fn show(s: String) {
    println!("{}", s);
}

fn main() {
    let opt = Some(String::from("hi"));
    match opt {
        Some(s) => show(s),
        None => {}
    }
}
