fn main() {
    let a = Some(1);
    let b = Some(String::from("b"));
    match a {
        Some(x) => {
            if let Some(y) = &b {
                println!("{} {}", x, y);
            }
        }
        None => {}
    }
    println!("{:?}", b);
}
