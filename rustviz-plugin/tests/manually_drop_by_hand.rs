// #283: dropping a `ManuallyDrop<T>` by hand. `ManuallyDrop::drop`
// drops the value inside it (md must not be used afterwards, and its
// scope end still drops nothing); `ManuallyDrop::take` moves the value
// out into the result, so md is moved-out afterwards.
use std::mem::ManuallyDrop;

struct Holder { a: ManuallyDrop<String>, b: String }

fn main() {
    let mut md = ManuallyDrop::new(String::from("x"));
    println!("{}", *md);
    unsafe { ManuallyDrop::drop(&mut md); }
    let mut m2 = ManuallyDrop::new(String::from("y"));
    let s = unsafe { ManuallyDrop::take(&mut m2) };
    println!("{}", s);
    let mut h = Holder { a: ManuallyDrop::new(String::from("a")), b: String::from("b") };
    unsafe { ManuallyDrop::drop(&mut h.a); }
    println!("{}", h.b);
}
