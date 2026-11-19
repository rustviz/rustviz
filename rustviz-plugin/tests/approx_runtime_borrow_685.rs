// #685: RefCell::borrow / borrow_mut, Mutex::lock and RwLock::read /
// write are drawn as the shared borrow they are at compile time. The
// run-time borrow or lock they take lasts until the guard is dropped and
// isn't drawn; the receiver's dot says so. `Borrow::borrow` on a String
// and `Cell::set` take no run-time borrow and get no note.
use std::borrow::Borrow;
use std::cell::{Cell, RefCell};
use std::sync::{Mutex, RwLock};

fn main() {
    let cell = RefCell::new(1);
    *cell.borrow_mut() += 1;
    let seen = *cell.borrow();
    let m = Mutex::new(0);
    *m.lock().unwrap() += 1;
    let l = RwLock::new(5);
    let r = *l.read().unwrap();
    *l.write().unwrap() += r;
    let s = String::from("s");
    let b: &str = s.borrow();
    let c = Cell::new(1);
    c.set(2);
    println!("{} {} {}", seen, b, c.get());
}
