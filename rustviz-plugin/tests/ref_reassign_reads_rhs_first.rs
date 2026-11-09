// `*r` is read (the condition) before r is given a new reference (#771).
fn main() {
    let a = 1;
    let b = 2;
    let c = 3;
    let mut r = &a;
    r = if *r > 0 { &b } else { &c };
    println!("{}", r);
}
