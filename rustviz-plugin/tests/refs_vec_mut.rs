fn main() {
    let mut x = 1;
    let mut y = 2;
    let slots = vec![&mut x, &mut y];
    println!("{:?}", slots);
    x += 1;
    println!("{} {}", x, y);
}
