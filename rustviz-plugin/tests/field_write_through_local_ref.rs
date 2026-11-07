// Writes through a local `&mut` to a struct (#152). The write is drawn
// on the reference's column (#147); the field being overwritten is
// the lender's (`s.a`), so that's where its old value is dropped. The
// same holds for a nested field, for a borrow of a field
// (`let q = &mut s.inner`), and for overwriting the whole struct
// (`*w = S { .. }` drops the old `s.a` too). Copy fields (`r.n`) have
// nothing to drop.
struct Inner {
    v: String,
}

struct S {
    a: String,
    n: i32,
    inner: Inner,
}

fn main() {
    let mut s = S { a: String::from("x"), n: 1, inner: Inner { v: String::from("v") } };
    let r = &mut s;
    r.a = String::from("y");
    r.n = 2;
    r.inner.v = String::from("u");
    let q = &mut s.inner;
    q.v = String::from("t");
    let w = &mut s;
    *w = S { a: String::from("z"), n: 3, inner: Inner { v: String::from("w") } };
    println!("{} {} {}", s.a, s.n, s.inner.v);
}
