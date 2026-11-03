use std::cell::RefCell;

fn main() {
    let c = RefCell::new(5);
    *c.borrow_mut() += 1;
    println!("{:?}", c);
}
