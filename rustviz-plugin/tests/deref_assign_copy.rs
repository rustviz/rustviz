fn main() {
    let mut x = 1;
    let y = 5;
    let w = &y;
    let r = &mut x;
    *r = y;
    *r = *w;
    *r = 9;
    println!("{}", x);
}
