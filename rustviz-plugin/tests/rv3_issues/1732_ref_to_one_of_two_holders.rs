fn first<'a>(v: &Vec<&'a String>) -> &'a String {
    v[0]
}
fn main() {
    let mut a = String::from("a");
    let b = String::from("b");
    let flag = std::env::args().count() > 1;
    let r: &String = &mut a;
    let x = vec![r];
    let y = vec![&b];
    let rx = if flag { &x } else { &y };
    let mut w: Vec<&String> = Vec::new();
    w.push(first(rx));
    println!("{:?}", w);
}
