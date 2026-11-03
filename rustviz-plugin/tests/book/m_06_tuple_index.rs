fn main() {
    let tup: (i32, f64, u8) = (500, 6.4, 1);
    let (x, y, z) = tup;
    let five_hundred = tup.0;
    println!("{} {} {} {}", x, y, z, five_hundred);
}
