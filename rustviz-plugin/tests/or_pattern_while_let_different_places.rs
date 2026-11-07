fn main() {
    let u = (String::from("c"), String::from("d"));
    while let (x, _) | (_, x) = u {
        println!("{}", x);
        break;
    }
}
