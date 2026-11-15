// A `static` gets a column in each function that uses it (#690). It
// exists before the function runs and is never dropped. Reading it
// copies out of it (`n`, `ox`, `p`), `&COUNT` borrows it, a method call
// borrows it, and a write to a `static mut` changes it. `primes` returns
// a reference to PRIMES, not a copy of it, so nothing is copied to the
// caller.
#![allow(static_mut_refs)]
use std::sync::atomic::{AtomicUsize, Ordering};

struct Pt { x: i32, y: i32 }
static COUNT: i32 = 3;
static ORIGIN: Pt = Pt { x: 0, y: 0 };
static PRIMES: [i32; 3] = [2, 3, 5];
static HITS: AtomicUsize = AtomicUsize::new(0);
static mut TOTAL: u32 = 0;

fn primes() -> &'static [i32; 3] {
    &PRIMES
}

fn main() {
    let n = COUNT;
    let r = &COUNT;
    let ox = ORIGIN.x;
    let p = PRIMES[1];
    HITS.fetch_add(1, Ordering::SeqCst);
    unsafe {
        TOTAL += 1;
    }
    println!("{} {} {} {} {} {:?}", n, r, ox, ORIGIN.y, p, primes());
}
