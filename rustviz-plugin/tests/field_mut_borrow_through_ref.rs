// Mutable borrows of a struct's fields through a `&mut` reference
// (#202): `&mut r.a` after `let r = &mut s;`, a nested field
// reborrowed twice (`&mut r.inner`, then `&mut q.v`), a method that
// takes `&mut self` on a field (`r.a.push_str(..)`), `&mut r.inner.v`
// passed to a function, and the same through a `&mut` parameter. Each
// is a reborrow drawn on the reference's column; a write through the
// new reference drops the old value on the owner's field column
// (`s.a`, also through a reborrow `let w2 = &mut *w; w2.a = …`), never
// on a reference (#323: `*r2 = …` after `let r2 = &mut *r1;`).
struct Inner {
    v: String,
}

struct S {
    a: String,
    n: i32,
    inner: Inner,
}

fn push(t: &mut String) {
    t.push_str("!");
}

fn edit(p: &mut S) {
    let f = &mut p.a;
    f.push_str("?");
    p.a.push_str("x");
    push(&mut p.inner.v);
}

fn main() {
    let mut s = S { a: String::from("a"), n: 1, inner: Inner { v: String::from("v") } };
    let r = &mut s;
    let f = &mut r.a;
    f.push_str("!");
    *f = String::from("b");
    r.n = 2;
    r.a.push_str("y");
    push(&mut r.inner.v);
    let q = &mut r.inner;
    let g = &mut q.v;
    *g = String::from("c");
    edit(&mut s);
    println!("{}", s.a);

    let mut t = String::from("t");
    let r1 = &mut t;
    let r2 = &mut *r1;
    *r2 = String::from("u");
    println!("{}", r2);

    let mut u = S { a: String::from("a"), n: 1, inner: Inner { v: String::from("v") } };
    let w = &mut u;
    let w2 = &mut *w;
    w2.a = String::from("w");
    println!("{}", u.a);
}
