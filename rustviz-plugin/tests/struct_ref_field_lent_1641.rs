// #1641: a struct borrowed as a whole lends its reference fields with it.
// While r is live, `*p.n = 2;` is E0506; main drew p.n "can read and write".
struct P<'a> { n: &'a mut i32 }
fn main() {
    let mut x = 0;
    let mut p = P { n: &mut x };
    let r = &mut p;
    *r.n = 1;
    *p.n = 2;
}

// A shared borrow of the struct: q.n can't be moved out or reborrowed mutably
// while r is live.
struct Q<'a> { n: &'a mut i32, m: &'a i32 }
fn shared(q: Q<'_>) {
    let r = &q;
    println!("{}", r.n);
    println!("{}", q.m);
}
