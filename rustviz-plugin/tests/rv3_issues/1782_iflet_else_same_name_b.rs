fn main() {
    let mut v = vec![String::from("a")];
    let other = String::from("o");
    if let Some(x) = v.first() {
        println!("{}", x);
    } else {
        let x = &other;
        v.push(String::new());
        println!("{}", x);
    }
}
