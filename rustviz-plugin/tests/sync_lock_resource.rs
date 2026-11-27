// std's lock types own an OS synchronization object. Whether they have
// drop glue depends on the target. Acquiring and borrowing a lock use
// resource wording (#484); only an actual destructor gets a drop triangle.

use std::sync::{Condvar, Mutex, RwLock};

struct Shared {
    count: Mutex<i32>,
}

fn main() {
    let m = Mutex::new(0);
    let g = m.lock();
    drop(g);

    let rw = RwLock::new(1);
    let r = rw.read();
    drop(r);

    let cv = Condvar::new();
    let sh = Shared { count: Mutex::new(2) };
}
