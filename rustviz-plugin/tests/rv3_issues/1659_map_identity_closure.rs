fn main() {
    let mut a = 1;
    let o = Some(&a);
    let r = o.map(|x| x);
    println!("{:?}", r);
    a = 2;
}
