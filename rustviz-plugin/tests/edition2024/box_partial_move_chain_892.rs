// #892: a let-chain on a Box's payload moves only b5.0 out of it, as
// `if let (s, n) = *b` does; n5 copies b5.1.
fn main() {
    let b5 = Box::new((String::from("e"), 5));
    if let (s5, n5) = *b5 && n5 > 0 {
        println!("{} {}", s5, n5);
    }
}
