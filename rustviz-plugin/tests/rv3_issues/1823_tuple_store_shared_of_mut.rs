fn main() {
    let a = String::from("a");
    let mut b = String::from("b");
    let mut v: Vec<(&String, &String)> = Vec::new();
    let r: &String = &mut b;
    v.push((&a, r));
    println!("{}", a);
    println!("{:?}", v);
}
