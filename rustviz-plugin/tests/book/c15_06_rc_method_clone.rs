use std::rc::Rc;

fn main() {
    let a = Rc::new(String::from("shared"));
    let b = a.clone();
    println!("{} {}", a, b);
}
