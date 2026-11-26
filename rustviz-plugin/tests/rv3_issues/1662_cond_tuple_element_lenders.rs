fn main() {
    let mut a = String::from("a");
    let b = String::from("b");
    let c = String::from("c");
    let flag = std::env::args().count() > 1;
    let r: &String = &mut a;
    let mut w: Vec<(&String, &String)> = Vec::new();
    w.push((if flag { r } else { &b }, &c));
    println!("{:?}", w);
}
