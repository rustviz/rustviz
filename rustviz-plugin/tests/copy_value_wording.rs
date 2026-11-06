// #274: a Copy value (i32, a Copy struct) is a "value" on its own
// column ("m holds a value"), so copying it, borrowing it and getting
// it back say "value" too, not "resource". A `&i32` that is copied
// still just holds a reference.
#[derive(Clone, Copy)]
struct P { x: i32 }

fn main() {
    let m = 3;
    let n = m;
    let r = &m;
    let r2 = r;
    println!("{} {} {} {}", m, n, r, r2);
    let mut k = 1;
    let w = &mut k;
    *w += 1;
    let f = || k + 1;
    println!("{}", f());
    let p = P { x: 1 };
    let q = p;
    println!("{} {}", p.x, q.x);
}
