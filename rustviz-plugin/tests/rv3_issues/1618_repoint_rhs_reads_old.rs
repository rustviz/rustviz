fn main() {
    let a = String::from("a");
    let b = String::from("b");
    let c = String::from("c");
    let x = &a;
    let mut r = x;
    let n = 1;
    r = if r.is_empty() { &b } else { &c };
    println!("{}", r);
}
