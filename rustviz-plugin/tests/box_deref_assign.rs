fn main() {
    let mut b = Box::new(5);
    *b += 1;
    println!("{}", b);
}
