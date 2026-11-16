fn main() {
    let x = Some(String::from("hello"));
    let opt = Some(5);
    match opt {
        Some(_) if let Some(a) = x && a.len() > 3 => println!("{}", a),
        _ => println!("other"),
    }
}
