use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::{Rc, Weak};
use std::sync::Arc;

struct Noisy(Rc<i32>);
impl Drop for Noisy {
    fn drop(&mut self) {}
}

fn main() {
    let a = Rc::new(5);
    let o = Some(Rc::clone(&a));
    let b = Box::new(Rc::clone(&a));
    let ao = Some(Arc::new(2));
    let wo = Some(Rc::downgrade(&a));
    let v = vec![Rc::clone(&a), Rc::clone(&a)];
    let mut h = HashMap::new();
    h.insert(1, Rc::clone(&a));
    let c = RefCell::new(Rc::clone(&a));
    let mixed = vec![(Rc::clone(&a), String::from("x"))];
    let keyed: HashMap<String, Rc<i32>> = HashMap::new();
    let n = Noisy(Rc::clone(&a));
    let d = Some(Rc::clone(&a));
    drop(d);
    println!("{} {} {} {} {}", o.is_some(), b, ao.is_some(), wo.is_some(), v.len());
    println!("{} {} {} {} {}", h.len(), c.borrow(), mixed.len(), keyed.len(), n.0);
}
