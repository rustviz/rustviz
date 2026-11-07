use std::rc::{Rc, Weak};

fn main() {
    let a = Rc::new(String::from("hi"));
    let w = Rc::downgrade(&a);
    let w2 = w.clone();
    let w3 = Weak::clone(&w);
    let s = w.upgrade().is_some();
    println!("{} {} {}", w2.strong_count(), w3.weak_count(), s);
}
