fn main() {
    let a = String::from("a");
    let mut b = String::from("b");
    let mut v: Vec<(&String, &mut String)> = Vec::new();
    v.extend([(&a, &mut b)]);
    println!("{}", v.len());
}
