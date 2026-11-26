fn main() {
    let mut a = String::from("a");
    let b = String::from("b");
    let mut r: &String = &mut a;
    let keep = r;
    let k = r; r = &b;
    println!("{} {} {}", keep, k, r);
}
