fn main() {
    let mut x = 10;
    let r = &mut x;
    *r += 1;
    let y = x;
    println!("{}", y);
}
