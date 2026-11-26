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

// Round 2 of the review.
struct R<'a> {
    r: &'a i32,
}

fn field_repoint_live() {
    let a = 1;
    let b = 2;
    let c = 3;
    let mut h = R { r: &a };
    let old = h.r;
    h.r = &b;
    black_box(h.r);
    h.r = &c;
    black_box(old);
    black_box(h.r);
}

fn field_repoint_from_var() {
    let a = 1;
    let b = 2;
    let mut h = R { r: &a };
    let s = &b;
    h.r = s;
    black_box(h.r);
}

fn drop_order(c: bool) {
    let mut t = (String::from("a"), String::from("b"), String::from("c"));
    drop(t.2);
    drop(t.1);
    if c {
        t.1 = String::new();
    }
}

fn synth_cond(c: bool) {
    let p = S { a: String::new(), b: String::new() };
    if c {
        drop(p.a);
    } else {
        drop(p);
    }
}

fn box_payload_moved() {
    let b = Box::new(String::from("b"));
    let s = *b;
    drop(s);
}

fn write_through(r: &mut String) {
    *r = String::from("new");
}

fn two_returns(c: bool) -> usize {
    let s = String::from("s");
    if c { return 0; } else { return s.len(); }
}

struct W(i32);

impl W {
    fn slot(&mut self) -> &mut i32 {
        &mut self.0
    }
}

fn activated_join(flag: bool) {
    let mut x = W(1);
    let mut y = 2;
    let r = if flag { x.slot() } else { &mut y };
    *r += 1;
}

fn deref_copy() -> i32 {
    let x = 1;
    let n = *&x;
    n
}

fn unique_capture(p: &mut i32) {
    let mut f = || *p = 1;
    f();
}

fn shadow_capture() {
    let x = String::from("x");
    let f = move || {
        let x = x;
        drop(x);
    };
    f();
}

fn factory() -> impl Fn(String) -> usize {
    |s| s.len()
}

fn call_returned() -> usize {
    let s = String::from("s");
    factory()(s)
}

fn make_pair() -> (String, String) {
    (String::new(), String::new())
}

fn ext_unnamed() {
    let _ = &String::from("u");
    let r = &make_pair().0;
    black_box(r);
}

enum E3 {
    A,
    B,
    C,
}

fn three_arms(e: E3) -> u8 {
    let n = match e { E3::A => 1, E3::B => 2, E3::C => 3 };
    n
}

fn inner_scope() {
    {
        let mut p = S { a: String::new(), b: String::new() };
        p.a = String::from("a");
        drop(p.a);
    }
    let z = 1;
    black_box(z);
}
