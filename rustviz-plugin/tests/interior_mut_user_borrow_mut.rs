// #203: the "checked at runtime" wording belongs to `RefCell::borrow_mut`
// only. It is chosen by the callee's identity, not its name, so a user
// type with its own `borrow_mut(&self)` mustn't get it. Both calls sit in
// one fixture so the RefCell one proves the sentence is still produced.
use std::cell::{Cell, RefCell};

struct Counter {
    hits: Cell<i32>,
}

impl Counter {
    fn borrow_mut(&self) -> i32 {
        self.hits.set(self.hits.get() + 1);
        self.hits.get()
    }
}

fn main() {
    let cell = RefCell::new(5);
    *cell.borrow_mut() += 1;
    let counter = Counter { hits: Cell::new(0) };
    let n = counter.borrow_mut();
    println!("{} {:?}", n, cell);
}
