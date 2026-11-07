fn main() {
    let mut x = 1;
    let mut y = 2;
    let mut slots = Vec::new();
    slots.push(&mut x);
    slots.push(&mut y);
    *slots[1] += 10;
    println!("{}", slots.len());
    x += 1;
    println!("{} {}", x, y);
}
