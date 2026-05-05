fn main () {
    let mut x = 7;
    let y = &mut x;
    let z = y;
    println!("x value {}", *z);
}