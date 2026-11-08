fn main() {
    let u = (String::from("c"), String::from("d"));
    if let (x, _) | (_, x) = u {
        println!("{}", x);
    }
}
