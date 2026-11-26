fn main() {
    let a = String::from("a");
    let b = String::from("b");
    let x = &a;
    let mut r = x;
    let noise = &a;
    println!("{}", noise);
    let z = &r;
    println!("{}", z);
    r = &b;
    println!("{}", r);
}
