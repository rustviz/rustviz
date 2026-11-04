// A closure that captures a Cell by immutable borrow can still change
// its contents (#133), so the call doesn't claim the body only reads.

use std::cell::Cell;

fn main() {
    let c = Cell::new(1);
    let f = || c.set(2);
    f();
    println!("{}", c.get());
}
