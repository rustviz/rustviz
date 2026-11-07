// #655: overwriting a struct *field* of a struct with another variable
// (`o.inner = other;`). The new value's reference field moves with it,
// so other.r's loan of t goes to o.inner.r, which gives t back at its
// last use (as `let hold = other;` does, #637). The write is partial:
// `o` stays live and its type has one lifetime for the old and the new
// reference, so rustc keeps s borrowed until o's last use too (a
// `s.push` between the write and the println! is E0502), as for
// `o.inner = mk(&t);` (#638) and `o.inner.r = &t;` (#531).
struct I<'a> { r: &'a String }
struct M<'a> { r: &'a mut String }
struct O<'a> { inner: I<'a>, n: i32 }
struct P<'a> { inner: M<'a>, n: i32 }
#[derive(Clone, Copy)]
struct C<'a> { r: &'a String }
struct Q<'a> { inner: C<'a> }

fn main() {
    let mut s = String::from("s");
    let t = String::from("t");
    let mut o = O { inner: I { r: &s }, n: 1 };
    let other = I { r: &t };
    o.inner = other;
    println!("{} {}", o.inner.r, o.n);
    s.push('!');
}

// A `&mut` field: u stays mutably borrowed until p's last use, v from
// the write on, through p.inner.r.
fn mutable() {
    let mut u = String::from("u");
    let mut v = String::from("v");
    let mut p = P { inner: M { r: &mut u }, n: 1 };
    let m = M { r: &mut v };
    p.inner = m;
    p.inner.r.push('!');
    println!("{}", p.n);
    println!("{} {}", u, v);
}

// A field of another struct moves the same way.
fn from_field() {
    let a = String::from("a");
    let b = String::from("b");
    let mut o = O { inner: I { r: &a }, n: 1 };
    let o2 = O { inner: I { r: &b }, n: 2 };
    o.inner = o2.inner;
    println!("{} {}", o.inner.r, o2.n);
}

// A Copy struct is copied: c and q.inner both hold the reference.
fn copied() {
    let e = String::from("e");
    let f = String::from("f");
    let mut q = Q { inner: C { r: &e } };
    let c = C { r: &f };
    q.inner = c;
    println!("{} {}", q.inner.r, c.r);
}
