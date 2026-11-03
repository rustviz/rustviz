use std::fmt::Display;

fn show<T: Display>(t: T) {
    println!("{}", t);
}

fn main() {
    let s = String::from("x");
    show(s);
    let n = 5;
    show(n);
    println!("{}", n);
}
