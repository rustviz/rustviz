fn main() {
    let t = (1, (Some(String::from("b")), Some(String::from("c"))));
    if let (n, (Some(x), _) | (_, Some(x))) = t {
        println!("{} {}", n, x);
    }
}
