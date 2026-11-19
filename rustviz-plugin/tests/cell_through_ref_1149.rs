// #1149: a `&self` method of Cell / RefCell called through a shared
// reference can change what the reference points to: it doesn't just
// read from it.
use std::cell::{Cell, RefCell};
fn bump(c: &Cell<i32>, r: &RefCell<String>) {
    c.set(42);
    r.borrow_mut().push('!');
}
fn add_one(k: &Cell<i32>) {
    k.set(k.get() + 1);
}
fn main() {
    let c = Cell::new(0);
    let r = RefCell::new(String::from("a"));
    bump(&c, &r);
    let rc = &c;
    rc.set(2);
    add_one(rc);
    println!("{}", c.get());
}
