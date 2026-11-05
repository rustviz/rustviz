// #203: `&self` methods on values with interior mutability. The borrow
// checker sees a shared borrow, but the value inside can change, so the
// call dot mustn't say "reads from". `RefCell::borrow_mut` checks
// exclusivity at runtime. A plain `&self` method still "reads from".
use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicUsize, Ordering};

fn main() {
    let c = Cell::new(1);
    let old = c.get();
    c.set(old + 1);
    let cell = RefCell::new(5);
    *cell.borrow_mut() += 1;
    let hits = AtomicUsize::new(0);
    hits.fetch_add(1, Ordering::SeqCst);
    let s = String::from("x");
    let n = s.len();
    println!("{} {:?} {:?} {}", c.get(), cell, hits, n);
}
