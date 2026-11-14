// #925: `&x` with x a reference variable borrows x itself: rr is a
// `&&String` that points to x, not a reborrow of what x points to
// (that's `&*x`). What rr borrows is the reference x holds, a value:
// a reference needs no drop. The shared counterpart of #914.
fn borrow_shared_ref() {
    let a = String::from("a");
    let x = &a;
    let rr = &x;
    println!("{}", rr);
}

// The same with a mutable reference m (k is a `&&mut String`): m is
// read-only while k is live, so `m.push` has to wait.
fn borrow_mut_ref() {
    let mut g = String::from("g");
    let m = &mut g;
    let k = &m;
    println!("{}", k);
    m.push('!');
}

// A reference to a Copy value: y borrows the reference p, a value too.
fn borrow_ref_to_copy() {
    let n = 5;
    let p = &n;
    let y = &p;
    println!("{}", y);
}

// A reborrow through x2: q points to b, not to x2.
fn reborrow() {
    let b = String::from("b");
    let x2 = &b;
    let q = &*x2;
    println!("{}", q);
}

// `&gx` coerced to a `&String` is a reborrow through gx too.
fn coerced_reborrow() {
    let c = String::from("c");
    let gx = &c;
    let s: &String = &gx;
    println!("{}", s);
}

fn main() {
    borrow_shared_ref();
    borrow_mut_ref();
    borrow_ref_to_copy();
    reborrow();
    coerced_reborrow();
}
