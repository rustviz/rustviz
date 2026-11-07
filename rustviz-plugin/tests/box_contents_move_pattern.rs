fn take(s: String) -> usize { s.len() }
fn main() {
    let pair = (Box::new(String::from("a")), 1);
    let (b, n) = pair;
    take(*b);
    println!("{}", n);
}
