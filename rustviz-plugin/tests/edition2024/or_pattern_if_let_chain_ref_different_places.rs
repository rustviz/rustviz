fn main() {
    let t = (Some(String::from("a")), Some(String::from("b")));
    if let (Some(ref x), _) | (_, Some(ref x)) = t && x.len() > 0 {
        println!("{}", x);
    }
    println!("{:?}", t);
}
