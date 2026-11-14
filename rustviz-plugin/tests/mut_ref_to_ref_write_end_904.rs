// #904: `*r = &t` through `r: &mut &T` stores a new reference in x, the
// variable r borrows. r's borrow of x ends at r's last use, as for any
// `&mut` (main drew a second, hollow line on r's column from the write
// to its scope end: "r ... can only read it"). The borrow of t goes to
// x's column, where it ends at x's last use. x's old loan of a lasts
// until then too: rustc rejects `a.push('!')` between the write and the
// `println!` (r borrows x, so x stays live across the write).

fn main() {
    let a = String::from("a");
    let t = String::from("t");
    let mut x = &a;
    let r = &mut x;
    *r = &t;
    println!("{}", x);
}

// w is used after the write: y is re-pointed while w still borrows it.
fn live_after() {
    let b = String::from("b");
    let u = String::from("u");
    let mut y = &b;
    let w = &mut y;
    *w = &u;
    println!("{}", w);
    println!("{}", y);
}

// z is not used after the write: the old loan of c ends at the write
// (`c.push('!')` right after compiles), and so does the new one of v.
fn dead_after() {
    let mut c = String::from("c");
    let v = String::from("v");
    let mut z = &c;
    let q = &mut z;
    *q = &v;
    c.push('!');
}

// The same with `&mut` references.
fn mutable() {
    let mut g = String::from("g");
    let mut h = String::from("h");
    let mut m = &mut g;
    let k = &mut m;
    *k = &mut h;
    m.push('!');
}

// The write on one path only.
fn in_branch() {
    let d = String::from("d");
    let e = String::from("e");
    let mut o = &d;
    let p = &mut o;
    if e.len() > 0 {
        *p = &e;
    }
    println!("{}", o);
}

// el points to an element: there is no variable to re-point, and the
// borrow of t1 is drawn into *el, but el's line still ends at its last
// use.
fn element() {
    let a1 = String::from("a1");
    let t1 = String::from("t1");
    let mut refs = vec![&a1];
    let el = &mut refs[0];
    *el = &t1;
    println!("{:?}", refs);
}
