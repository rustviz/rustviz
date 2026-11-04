// #277: a closure that uses a reference variable through it captures
// the place `*r`: it reborrows what r points to (s / n), it doesn't
// borrow a "resource" of r's own. A `&mut` r can't be used while the
// closure is live and is usable again after its last call; a closure
// that only reads through r leaves r read-only meanwhile. `t` (not a
// reference) is still an ordinary capture.
fn main() {
    let mut s = String::from("a");
    let r = &mut s;
    let mut f = || r.push('b');
    f();
    r.push('c');
    let len = || r.len();
    println!("{}", len());
    let mut n = 1;
    let p = &mut n;
    let mut g = || *p += 1;
    g();
    let mut t = String::new();
    let mut h = || t.push('x');
    h();
    println!("{} {} {}", s, n, t);
}
