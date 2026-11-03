fn main() {
    let mut n = 0;
    let r = &mut n;
    *r += 1;
    println!("{}", n);
}
