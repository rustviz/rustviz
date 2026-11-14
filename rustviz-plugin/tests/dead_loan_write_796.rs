// #796: a write to a lender on a path where its loan is already dead.
// The diagram gives each loan one end, the latest over the paths, as
// the borrow checker does (#778), so the lender is drawn borrowed across
// the write; the write's dot says why rustc accepts it. Without the
// reassignment of the reference on the write's path (in `returns`, the
// `return`), rustc rejects each write (E0506; E0502 for `v[0] = 7`).
// See dead_loan_write_near_796.rs for writes that get no note.
struct P {
    n: i32,
}

// The reference is given a new one after the write.
fn after() {
    let mut x = 8;
    let y = 9;
    let mut c = &x;
    if y > 5 {
        x = 2;
        c = &y;
    }
    println!("{}", c);
}

// ...or before it.
fn before() {
    let mut a = 8;
    let b = 9;
    let mut r = &a;
    if b > 5 {
        r = &b;
        a += 1;
    }
    println!("{}", r);
}

// In the `else` arm; the `if` arm reads the old reference.
fn in_else() {
    let mut d = 8;
    let e = 9;
    let mut q = &d;
    if e > 5 {
        println!("{}", q);
    } else {
        d = 2;
        q = &e;
    }
    println!("{}", q);
}

// A `&mut` loan, in a `match` arm.
fn mut_in_match() {
    let mut m = 8;
    let mut k = 9;
    let mut p = &mut m;
    match k {
        0 => *p += 1,
        _ => {
            m = 2;
            p = &mut k;
        }
    }
    *p += 1;
}

// A field of the lender, and an element of it.
fn parts() {
    let mut s = P { n: 1 };
    let t = P { n: 2 };
    let mut f = &s;
    let mut v = vec![1, 2];
    let w = vec![3];
    let mut g = &v;
    if t.n > 1 {
        s.n = 5;
        f = &t;
        v[0] = 7;
        g = &w;
    }
    println!("{} {:?}", f.n, g);
}

// On the path that returns, the reference isn't used again.
fn returns(z: i32) {
    let mut h = 8;
    let u = &h;
    if z > 5 {
        h = 2;
        println!("{}", h);
        return;
    }
    println!("{}", u);
}

fn main() {
    after();
    before();
    in_else();
    mut_in_match();
    parts();
    returns(1);
}
