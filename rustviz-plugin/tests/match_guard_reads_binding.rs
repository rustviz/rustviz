fn main() {
    let opt = Some(String::from("hello"));
    match opt {
        Some(t) if t.len() > 2 => {
            println!("long {}", t);
        }
        Some(t) => println!("short {}", t),
        None => {}
    }
}
