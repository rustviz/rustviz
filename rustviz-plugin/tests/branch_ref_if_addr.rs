fn main() {
    let a = String::from("a");
    let b = String::from("b");
    let r = if a.len() > 0 { &a } else { &b };
    println!("{}", r);
}
