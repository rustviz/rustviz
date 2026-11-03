// Cell / RefCell / Mutex methods take `&self` and mutate through it,
// so an immutable borrow of them does not make the owner read-only.
// A plain String lent immutably is read-only for comparison.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Mutex;

fn main() {
    let c = Cell::new(1);
    c.set(2);

    let r = RefCell::new(String::from("r"));
    r.borrow_mut().push_str("!");

    let shared = Rc::new(RefCell::new(5));
    let n = shared.borrow();

    let m = Mutex::new(0);
    let g = m.lock();

    let s = String::from("s");
    let len = s.len();
}
