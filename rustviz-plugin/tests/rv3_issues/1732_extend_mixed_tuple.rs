fn main() {
    let mut a = String::from("a");
    let mut b = String::from("b");
    let r: &String = &mut a;
    let mut v: Vec<(&String, &mut String)> = Vec::new();
    v.extend([(r, &mut b)]);
    println!("{}", v.len());
}
