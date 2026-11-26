fn choose<'a>(x: &'a mut String, y: &'a String, f: bool) -> &'a String {
    if f { x } else { y }
}
fn id(r: &String) -> &String {
    r
}
fn main() {
    let mut a = String::from("a");
    let b = String::from("b");
    let flag = true;
    let mut v: Vec<&String> = Vec::new();
    let r = choose(&mut a, &b, flag);
    v.push(id(r));
    println!("{:?}", v);
}
