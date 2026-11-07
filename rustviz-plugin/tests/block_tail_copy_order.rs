fn main() {
    let m = 3;
    let k = { let t = m * 2; t };
    println!("{}", k);
}
