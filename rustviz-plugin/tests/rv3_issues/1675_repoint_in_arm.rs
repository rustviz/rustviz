fn main() {
    let x = String::from("x");
    let y = String::from("y");
    let mut c = &x;
    if x.len() > 0 {
        c = &y;
        println!("in");
    }
    println!("{}", c);
}
