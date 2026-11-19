#![allow(unreachable_code)]
// An assignment whose right-hand side never finishes (#1142): `return`,
// `panic!`, `break`, `std::process::exit`. The right-hand side runs
// first, so the assignment never happens: no new value, and the old one
// is dropped (if at all) by the exit, not by the assignment.

struct P {
    f: String,
}

fn ret(c: bool) {
    let mut a = String::from("a");
    if c {
        a = return;
    }
    println!("{}", a);
}

fn panics(c: bool) {
    let mut b = String::from("b");
    if c {
        b = panic!("stop");
    }
    println!("{}", b);
}

fn breaks(v: Vec<i32>) {
    let mut d = String::from("d");
    for _x in v {
        d = break;
    }
    println!("{}", d);
}

fn exits(c: bool) {
    let mut e = String::from("e");
    if c {
        e = std::process::exit(1);
    }
    println!("{}", e);
}

fn field(c: bool) {
    let mut p = P { f: String::from("f") };
    if c {
        p.f = return;
    }
    println!("{}", p.f);
}

fn through_ref(c: bool) {
    let mut u = String::from("u");
    let r = &mut u;
    if c {
        *r = return;
    }
    println!("{}", u);
}

fn main() {
    ret(false);
    panics(false);
    breaks(vec![]);
    exits(false);
    field(false);
    through_ref(false);
}
