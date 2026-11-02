use std::cell::RefCell;
use std::rc::Rc;

fn main() {
    let value = Rc::new(RefCell::new(5));
    let a = Rc::clone(&value);
    *value.borrow_mut() += 10;
    println!("{:?}", a);
}
