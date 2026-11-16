fn main() {
    let s = String::from("hello");
    let f = move || { drop(s); true };
    let opt = Some(5);
    match opt {
        Some(_) if f() => println!("long"),
        _ => println!("other"),
    }
}
