fn main() {
    let mut s = String::from("hello");
    let r = &mut s;
    let r2 = r;
    r2.push_str("!");
    println!("{}", s);
}
