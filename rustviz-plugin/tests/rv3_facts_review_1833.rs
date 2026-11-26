// RustViz 3 fact layer: the cases the review of #1833 found, pinned by
// `facts_pin_review_1833` in rustviz-lib/tests/corpus.rs.
use std::hint::black_box;

struct H<'a> {
    r: &'a mut i32,
    n: i32,
}

fn why_unrelated(cell: &mut i32) -> i32 {
    let r = &mut *cell;
    let unrelated = 7;
    black_box(unrelated);
    *r
}

fn both_arms(c: bool) {
    let mut x = 1;
    let r = &mut x;
    if c { black_box(*r); } else { black_box(*r); }
    let _unused = &mut x;
    x += 1;
}

fn copy_then_end() -> i32 {
    let x = 5;
    let r = &x;
    let k = *r;
    k
}

fn copied_holder() -> usize {
    let x = String::from("x");
    let r = &x;
    let n = r.len();
    let s = r;
    n + s.len()
}

fn two_phase_rows() {
    let mut v = vec![1];
    v.push(
        v.len(),
    );
}

fn field_holder_far() {
    let mut n = 1;
    let h = H {
        r: &mut n,


        n: 2,
    };
    *h.r += h.n;
    let q = &mut *h.r;
    *q += 1;
}

fn field_repoint() {
    let mut a = 1;
    let mut b = 2;
    let mut h = H { r: &mut a, n: 0 };
    h.r = &mut b;
    *h.r += h.n;
}

fn try_tail(o: Option<u8>) -> Option<usize> {
    let s = String::from("s");
    let v = o?;
    Some(s.len() + v as usize)
}

fn break_arm(c: bool) {
    let mut n = 0;
    loop {
        let t = String::from("t");
        if c { break; } else { n += t.len(); }
        if n > 3 { break; }
    }
}

fn ext_one_line() {
    { let r = &String::from("e"); black_box(r); }
}

fn add_copies() -> i32 {
    let a = 1;
    let b = 2;
    let c = a + b;
    c
}

fn declared_later(c: bool) -> String {
    let s: String;
    if c { s = String::from("a"); } else { s = String::from("b"); }
    s
}

fn arm_loans(c: bool) -> usize {
    let x = String::from("x");
    let y = String::from("y");
    let r;
    if c { r = &x; } else { r = &y; }
    r.len()
}

fn closures() -> i32 {
    let outer = || {
        let t = 1;
        let t = t + 1;
        let inner = || t * 2;
        inner()
    };
    outer()
}

struct S {
    a: String,
    b: String,
}

fn field_reinit(mut p: S, flag: bool) {
    let x = p.a;
    if flag {
        p.a = String::new();
    }
    drop(x);
}

fn call_arm(c: bool) -> usize {
    let n;
    if c { n = 1usize.max(2) } else { n = 3usize.min(4) }
    n
}
