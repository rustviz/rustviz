// RustViz 3 P1-3b (#1870's Codex review): an owner's title while a part of
// it is lent, where who holds the loan, whether it runs, or its kind is
// less simple than `let r = &mut p.a;`. Pinned by
// `part_loan_titles_say_what_rustc_counts` in rustviz-lib/tests/corpus.rs.
#![allow(unreachable_patterns, unused)]
struct P { a: String, b: String }
struct S<'a> { r: &'a mut i32, x: String }
struct R<'a> { r: &'a mut String }
struct O<'a> { v: Option<&'a mut String> }

fn read(p: &R) -> String { p.r.clone() }
fn pick<'a>(a: &'a mut String, b: &'a mut String) -> &'a String { a }

// r may point to other after the `if`: rustc still counts p.0 as borrowed.
fn overwritten(flag: bool) {
    let mut p = (String::new(), 1);
    let other = String::new();
    let mut r = &p.0;
    if flag { r = &other; }
    println!("{r}");
}

// The arm that borrows p.0 can't run; rustc counts the loan anyway.
fn unreachable_arm(n: i32) {
    let p = (String::new(), 1);
    let other = String::new();
    let r = match n { _ => &other, 1 => &p.0 };
    println!("{r}");
}

// g captures s.r uniquely (writes through it) and s.x mutably.
fn mixed_captures() {
    let mut n = 0;
    let mut s = S { r: &mut n, x: String::new() };
    let mut g = || { let _alias = &s.r; *s.r += 1; s.x.push('x'); };
    g();
    println!("{}", s.x);
}

// r is a `&String` whose lifetime keeps both mutable loans live.
fn indirect_holder() {
    let mut p = P { a: String::new(), b: String::new() };
    let r = pick(&mut p.a, &mut p.b);
    println!("{r}");
}

// A free function's `&mut` argument is a two-phase borrow too.
fn reserved_by_free_fn() {
    let mut s = String::new();
    let p = R { r: &mut s };
    let _old = std::mem::replace(
        p.r,
        read(&p),
    );
}

// y keeps e.0 shared and e.1 mutable: e.0 can still be read.
fn mixed_kinds() {
    let mut e = (1, 2);
    let mut y: Vec<(&i32, &mut i32)> = Vec::new();
    y.extend([(&e.0, &mut e.1)]);
    let a = e.0;
    println!("{}", y.len());
}

// x borrows the String s.v's payload points to.
fn deref_payload() {
    let mut t = String::from("t");
    let s = O { v: Some(&mut t) };
    if let Some(&mut ref x) = s.v {
        println!("{x}");
    }
}

// r borrows p.a on one path and q.a on the other.
fn two_lenders(c: bool) {
    let mut p = P { a: String::new(), b: String::new() };
    let mut q = P { a: String::new(), b: String::new() };
    let r = if c { &mut p.a } else { &mut q.a };
    r.clear();
}

fn main() {}
