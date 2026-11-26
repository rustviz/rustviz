fn main() {
    let mut a = String::from("a");
    let b = String::from("b");
    let mut x: &String = &mut a;
    let y = x; x = &b; x = y; let k = x;
    println!("{} {}", k, y);
}
