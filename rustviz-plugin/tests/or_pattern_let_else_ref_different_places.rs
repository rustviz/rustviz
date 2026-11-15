fn main() {
    let t = (Some(String::from("a")), Some(String::from("b")));
    let ((Some(ref x), _) | (_, Some(ref x))) = t else { return };
    println!("{}", x);
    println!("{:?}", t);
}
