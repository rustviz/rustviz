// A write through a reference to a local struct (#854) is also drawn on
// the struct's field column (`s.n`, `s.a`), not only on the reference's.
struct Inner {
    n: i32,
    v: String,
}

struct S {
    n: i32,
    a: String,
    inner: Inner,
}

fn bump(c: &mut S) {
    c.n = 5;
}

fn main() {
    let mut s = S { n: 1, a: String::new(), inner: Inner { n: 0, v: String::new() } };
    let r = &mut s;
    r.n = 2;
    bump(r);
    let flag = s.n > 0;
    let r1 = &mut s;
    if flag {
        r1.n = 3;
        r1.a = String::from("b");
    } else {
        r1.n = 4;
    }
    let r2 = &mut s;
    match flag {
        true => r2.n = 6,
        false => r2.a = String::from("c"),
    }
    let r3 = &mut s;
    let r4 = &mut *r3;
    r4.n = 7;
    r4.a = String::from("d");
    let r5 = &mut s;
    (*r5).n = 8;
    let r6 = &mut s;
    r6.inner.n = 9;
    r6.inner.v = String::from("e");
    let r7 = &mut s;
    r7.n += 1;
    println!("{}", s.n);
}
