fn main() {
    let mut t = (Some(String::from("a")), Some(String::from("b")));
    while let (Some(ref x), _) | (_, Some(ref x)) = t {
        println!("{}", x);
        t = (None, None);
    }
}
