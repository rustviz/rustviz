// #191: a union never drops its fields, at scope end or on a write.
// The String inside `slot.s` is dropped by hand; without that line it
// would leak. Writing `slot.n` overwrites the bytes and drops nothing.
use std::mem::ManuallyDrop;

union Slot { n: u32, s: ManuallyDrop<String> }

fn main() {
    let mut slot = Slot { s: ManuallyDrop::new(String::from("hi")) };
    unsafe { ManuallyDrop::drop(&mut slot.s); }
    slot.n = 5;
    let k = unsafe { slot.n };
    println!("{}", k);
}
