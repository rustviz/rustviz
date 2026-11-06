// Borrowing from a reference borrows what it points to, through it
// (#313): r owns nothing, so "r's resource is mutably borrowed by r2"
// was wrong. While a mutable reborrow (`&mut *r`, `r.push(..)`,
// `add(p)`) is live r can't be used; during a shared one (`&*r`) a
// `&mut` r can only read. A reference to an i32 points to a "value".

fn add(n: &mut i32) {
    *n += 1;
}

fn main() {
    let mut s = String::new();
    let r = &mut s;
    let r2 = &mut *r;
    r2.push('a');
    r.push('b');
    let r3 = &*r;
    println!("{}", r3);

    let mut x = 1;
    let p = &mut x;
    let q = &mut *p;
    *q += 1;
    add(p);
    *p += 1;
    println!("{}", x);
}
