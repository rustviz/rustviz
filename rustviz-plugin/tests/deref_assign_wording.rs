// An assignment `*r = e` to a place behind a reference or a Box reads
// like a plain assignment `y = e` (2026-11-05 decision): the subject is
// the place `*r`, a Copy value is "assigned a new value", and an owned
// value says its previous resource is dropped. The arrow carries the
// copy or move. An in-place update (`*r += 1`) keeps "modified through".
struct Q {
    n: i32,
}

fn main() {
    let mut x = 1;
    let y = 2;
    let r = &mut x;
    *r = y;
    *r = 5;
    *r += 1;
    let mut a = String::from("a");
    let t = String::from("t");
    let ra = &mut a;
    *ra = t;
    let mut b = Box::new(String::from("b"));
    let u = String::from("u");
    *b = u;
    let mut q = Q { n: 1 };
    let q2 = Q { n: 2 };
    let rq = &mut q;
    *rq = q2;
    println!("{} {} {} {}", x, a, b, q.n);
}
