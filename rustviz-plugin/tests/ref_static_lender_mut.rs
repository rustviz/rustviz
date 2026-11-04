// The mutable form: `r` mutably borrows the `static mut` COUNTER (allowed
// here so the 2024 edition's static_mut_refs lint doesn't reject it), and
// the borrow-end arrow names the static.
#![allow(static_mut_refs)]
static mut COUNTER: i32 = 0;

fn main() {
    let r = unsafe { &mut COUNTER };
    *r += 1;
    println!("{}", r);
}
