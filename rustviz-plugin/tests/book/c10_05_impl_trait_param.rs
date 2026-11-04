use std::fmt::Display;

fn notify(item: &impl Display) {
    println!("Breaking news! {}", item);
}

fn main() {
    let s = String::from("x");
    notify(&s);
    println!("{}", s);
}
