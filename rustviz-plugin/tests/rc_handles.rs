use std::rc::{Rc, Weak};
use std::sync::Arc;

fn main() {
    let a = Rc::new(String::from("hi"));
    let b = Rc::clone(&a);
    let c = a.clone();
    let w: Weak<String> = Rc::downgrade(&a);
    let s = Arc::new(5);
    let mut t = Arc::clone(&s);
    t = Arc::new(6);
    println!("{} {} {} {}", b, c, s, t);
}
