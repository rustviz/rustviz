fn main() {
    let t = (Some(String::from("a")), Some(String::from("b")));
    if let (Some(x), _) | (_, Some(x)) = t {
        println!("{}", x);
    } else {
        println!("none");
    }
}
