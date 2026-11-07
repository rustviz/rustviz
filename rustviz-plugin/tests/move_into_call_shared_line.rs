fn take(s: String) -> usize { s.len() }
fn main() {
    let a = String::from("a");
    let b = String::from("b");
    let c = a; let n = take(b);
    println!("{} {}", c, n);
}
