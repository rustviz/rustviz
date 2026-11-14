fn main() {
    let m = 3;
    let k = { let t = m * 2; t };
    let (k2, q) = ({ let u = m * 2; u }, m);
    println!("{} {} {}", k, k2, q);
}
