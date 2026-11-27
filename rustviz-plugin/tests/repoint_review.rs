fn read(value: &String) { println!("{value}"); }
fn pick<'a>(a: &'a String, b: &'a String, flag: bool) -> &'a String {
    if flag { a } else { b }
}
fn choice(flag: bool) {
    let a = String::from("a");
    let b = String::from("b");
    let c = String::from("c");
    let mut r = &c;
    read(r);
    r = pick(&a, &b, flag); read(r); r = &c; read(r);
}
fn independent() {
    let a = String::from("a");
    let mut s = String::from("s");
    let mut t = String::from("t");
    let held = &mut t;
    let mut r = &a;
    read(r);
    r = &s; read(r); s.clear(); r = &s; read(r);
    held.push('!');
}
fn inspect(v: &i32) { println!("{v}"); }
fn writes() {
    let a = 3;
    let mut s = 4;
    let mut n = 0;
    let mut r = &a;
    inspect(r);
    r = &s; inspect(r); inspect(&n); n = 1; inspect(&n); s += 1; r = &s; inspect(r);
}
struct Holder<'a> { r: &'a String }
fn aggregate() {
    let a = String::from("a");
    let b = String::from("b");
    let c = String::from("c");
    let mut h = Pair { r: &a, sibling: &c };
    let other = Pair { r: &b, sibling: &c };
    read(h.r); read(h.sibling);
    h = other;
    read(h.r); read(h.sibling);
    h.r = &c;
    read(h.r); read(h.sibling);
}
fn moved() {
    let mut a = 0;
    let mut b = 1;
    let mut c = 2;
    let mut r = &mut a;
    *r += 1;
    r = &mut b;
    let q = r;
    r = &mut c;
    *r += 1;
    *q += 1;
}
fn maybe(flag: bool) {
    let a = String::from("a");
    let b = String::from("b");
    let mut r;
    if flag { r = &a; read(r); }
    r = &b;
    read(r);
}
fn indirect() {
    let a = 11;
    let b = 22;
    let c = 33;
    let mut r = &a;
    let p = &raw mut r;
    r = &b;
    let before = *r;
    unsafe { *p = &c; }
    let after = *r;
    r = &a;
    inspect(&before); inspect(&after); inspect(r);
}
fn field_copy() {
    let a = String::from("a");
    let b = String::from("b");
    let c = String::from("c");
    let mut h = Holder { r: &a };
    read(h.r);
    h.r = &b; let q = h.r; h.r = &c; read(h.r);
    read(q);
}
fn main() {
    choice(true);
    choice(false);
    independent();
    writes();
    aggregate();
    moved();
    maybe(true);
    maybe(false);
    indirect();
    field_copy();
    still_shared();
}

struct Pair<'a> { r: &'a String, sibling: &'a String }

fn still_shared() {
    let a = String::from("a");
    let mut s = String::from("s");
    let held = &s;
    let mut r = &a;
    read(r);
    r = &s; read(r); r = &s; read(r);
    read(held);
    s.clear();
}
