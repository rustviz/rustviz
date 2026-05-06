// Reborrow: a fresh `&mut *r` from an existing mutable reference.
// The reborrow is its own borrow with its own lifetime; the original
// reference is loaned out for the duration of the reborrow and gets
// it back when the reborrow expires.

fn main() {
    let mut s = String::from("hi");
    let r = &mut s;
    let r2 = &mut *r;
    r2.push_str("!");
    println!("{}", r);
}
