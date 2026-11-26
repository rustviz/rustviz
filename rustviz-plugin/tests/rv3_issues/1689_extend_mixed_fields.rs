fn main() {
    let mut e = (String::from("a"), String::from("b"));
    let mut y: Vec<(&String, &mut String)> = Vec::new();
    y.extend([(&e.0, &mut e.1)]);
    println!("{}", y.len());
}
