fn main () {
    let mut x = 7;
    let y = &x;
    let z = &x;
    let c = *y + *z;
    x += 6;
    println!("x value {}", c);
}