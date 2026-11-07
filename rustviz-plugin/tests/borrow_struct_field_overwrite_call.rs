// #638: overwriting a struct *field* of a struct with a call's result
// (`o.inner = mk(&t);`). The new value's reference field borrows what
// mk's signature ties it to, so o.inner.r borrows t from the write
// until its last use (main drew no borrow of t, and handed o.inner.r's
// borrow back to s). The write is partial: `o` stays live and its type
// has one lifetime for the old and the new reference, so rustc keeps s
// borrowed until o's last use too, as for `o.inner = I { r: &f };` and
// `o.inner.r = &f;` (#531).
struct I<'a> { r: &'a String }
struct M<'a> { r: &'a mut String }
struct O<'a> { inner: I<'a>, n: i32 }
struct P<'a> { inner: M<'a>, n: i32 }
struct W { s: String }

fn mk<'a>(r: &'a String) -> I<'a> { I { r } }

impl<'a> M<'a> {
    fn new(r: &'a mut String) -> M<'a> { M { r } }
}

impl W {
    fn view(&self) -> I<'_> { I { r: &self.s } }
}

fn main() {
    let mut s = String::from("s");
    let t = String::from("t");
    let mut o = O { inner: I { r: &s }, n: 1 };
    o.inner = mk(&t);
    println!("{} {}", o.inner.r, o.n);
    s.push('!');
}

// A `&mut` field, overwritten with an associated function's result:
// u stays mutably borrowed until p's last use, v from the write on.
fn mutable() {
    let mut u = String::from("u");
    let mut v = String::from("v");
    let mut p = P { inner: M { r: &mut u }, n: 1 };
    p.inner = M::new(&mut v);
    p.inner.r.push('!');
    println!("{}", p.n);
    println!("{}", u);
}

// A method call's result: o.inner.r borrows w (through `&self`).
fn method() {
    let a = String::from("a");
    let w = W { s: String::from("w") };
    let mut o = O { inner: I { r: &a }, n: 1 };
    o.inner = w.view();
    println!("{} {}", o.n, o.inner.r);
}

// A struct literal's field initialised by a call borrows the same way.
fn initialised() {
    let b = String::from("b");
    let o = O { inner: mk(&b), n: 1 };
    println!("{} {}", o.n, o.inner.r.len());
}
