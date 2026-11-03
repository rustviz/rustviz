fn main() {
    let mut x = 1;
    let y = 5;
    let r = &mut x;
    *r = y;
    println!("{}", x);
}
