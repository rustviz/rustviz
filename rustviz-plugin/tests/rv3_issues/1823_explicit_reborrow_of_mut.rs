fn main() {
    let mut c = String::from("c");
    let mut u: Vec<&String> = Vec::new();
    let rc: &String = &mut c;
    u.push(&*rc);
    println!("{:?}", u);
}
