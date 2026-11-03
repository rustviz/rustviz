use std::rc::Rc;

fn main() {
    let a = Rc::new(5);
    let b = Rc::clone(&a);
    println!("{} {}", Rc::strong_count(&a), b);
}
