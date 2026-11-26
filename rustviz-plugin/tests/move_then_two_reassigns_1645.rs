// #1645: `drop(s)` empties s, the first assignment gives it "b" (nothing
// to drop), and the second drops "b" before storing "c".
fn main() {
    let mut s = String::from("a");
    drop(s); s = String::from("b"); s = String::from("c");
    println!("{}", s);
}
