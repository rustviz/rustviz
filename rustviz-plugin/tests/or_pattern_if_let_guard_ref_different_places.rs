fn main() {
    let t = (Some(String::from("a")), Some(String::from("b")));
    let o = Some(1);
    match o {
        Some(n) if let (Some(ref x), _) | (_, Some(ref x)) = t => {
            println!("{} {}", n, x);
        }
        _ => {}
    }
    println!("{:?}", t);
}
