fn main() {
    let s = String::from("hello");
    let t: &str = s.as_str();
    println!("{}", t);
}
