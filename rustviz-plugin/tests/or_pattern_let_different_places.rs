fn main() {
    let u = (String::from("c"), String::from("d"));
    let ((x, _) | (_, x)) = u;
    println!("{}", x);
}
