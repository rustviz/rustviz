// #283: a `ManuallyDrop<T>` never drops the `T` inside it, at scope end
// or when it's overwritten. These Strings are leaked; a String moved
// out with `into_inner` is the caller's to drop. A `ManuallyDrop`
// struct field behaves the same; the String field next to it is
// dropped as usual.
use std::mem::ManuallyDrop;

struct Holder { a: ManuallyDrop<String>, b: String }

fn keep(md: ManuallyDrop<String>) {
    println!("{}", *md);
}

fn main() {
    let mut md = ManuallyDrop::new(String::from("x"));
    md = ManuallyDrop::new(String::from("y"));
    println!("{}", *md);
    let md2 = ManuallyDrop::new(String::from("z"));
    let s = ManuallyDrop::into_inner(md2);
    keep(ManuallyDrop::new(s));
    let h = Holder { a: ManuallyDrop::new(String::from("a")), b: String::from("b") };
    println!("{} {}", *h.a, h.b);
}
