// #706: only a type that contains an `UnsafeCell` (Cell, RefCell,
// Mutex, atomics) has interior mutability. A `dyn Fn()` hides what
// its closure captured, so a `Vec<Box<dyn Fn()>>` is said neither to
// have interior mutability nor to be read-only while borrowed.

use std::cell::Cell;

fn main() {
    let s = String::from("s");
    let mut fns: Vec<Box<dyn Fn() -> usize + '_>> = Vec::new();
    fns.push(Box::new(|| s.len()));
    println!("{}", fns.len());
    let r = &fns;
    println!("{}", r.len());
    let c = Cell::new(1);
    c.set(2);
    let cells = vec![Cell::new(1)];
    println!("{} {}", c.get(), cells.len());
}
