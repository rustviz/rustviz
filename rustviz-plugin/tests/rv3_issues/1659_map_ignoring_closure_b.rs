fn main() {
    let mut a = 1;
    let b = 2;
    let o = Some(&a);
    let r = o.map(|_| &b);
    a = 2;
    println!("{:?}", r);
}
