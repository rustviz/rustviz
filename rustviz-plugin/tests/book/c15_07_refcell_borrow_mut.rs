use std::cell::RefCell;

fn main() {
    let c = RefCell::new(vec![1]);
    c.borrow_mut().push(2);
    println!("{}", c.borrow().len());
}
