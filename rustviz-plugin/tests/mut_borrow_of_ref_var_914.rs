// #914: `&mut m` with m a reference variable borrows m itself: k is a
// `&mut &mut String` that points to m, not a reborrow of what m points
// to (that's `&mut *m`). What k borrows is the reference m holds, a
// value: a reference needs no drop.
fn borrow_mut_ref() {
    let mut g = String::from("g");
    let mut m = &mut g;
    let k = &mut m;
    k.push('!');
    m.push('?');
}

// The same with a shared reference x (r is a `&mut &String`).
fn borrow_shared_ref() {
    let a = String::from("a");
    let mut x = &a;
    let r = &mut x;
    println!("{}", r);
    println!("{}", x);
}

// A reborrow through m2: q points to h, not to m2.
fn reborrow() {
    let mut h = String::from("h");
    let m2 = &mut h;
    let q = &mut *m2;
    q.push('!');
    m2.push('?');
}

// `&mut gm` coerced to a `&mut String` is a reborrow through gm too.
fn coerced_reborrow() {
    let mut c = String::from("c");
    let mut gm = &mut c;
    let s: &mut String = &mut gm;
    s.push('#');
    gm.push('$');
}

fn main() {
    borrow_mut_ref();
    borrow_shared_ref();
    reborrow();
    coerced_reborrow();
}
