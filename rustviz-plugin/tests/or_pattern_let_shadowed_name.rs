fn main() {
    let x = String::from("z");
    println!("{}", x);
    let u = (String::from("a"), String::from("b"));
    if let (x, _) | (_, x) = u {
        println!("{}", x);
    }
    let t = (String::from("c"), String::from("d"));
    let ((x, _) | (_, x)) = t;
    println!("{}", x);
}
