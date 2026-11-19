// A `ManuallyDrop<T>` never drops the `T` inside it (#283): it has no
// drop glue, so Rust keeps no drop flag for it. Moved (or given its
// value) on some paths only, it still drops nothing at its scope end,
// and the join after the branches has no "its resource is dropped when
// ... goes out of scope" to tell (#1147). The paths: an `if`, a `match`
// arm, a let-chain condition, a `break` in a loop, a `let x;` assigned
// in an `if` or a loop. A struct whose String field was moved out drops
// nothing more: its ManuallyDrop field drops nothing, and a field with
// nothing to drop that some branch moved needs no drop flag either.

use std::mem::ManuallyDrop;

struct NoDrop(i32);

struct S {
    a: String,
    n: NoDrop,
    m: ManuallyDrop<String>,
}

fn take(_m: ManuallyDrop<String>) -> bool {
    true
}

fn take_n(_n: NoDrop) {}

fn in_if(c: bool) {
    let md = ManuallyDrop::new(String::from("md"));
    if c {
        let _s = ManuallyDrop::into_inner(md);
    }
}

fn in_arm(o: Option<i32>) {
    let ma = ManuallyDrop::new(String::from("ma"));
    match o {
        Some(_) => {
            take(ma);
        }
        None => {}
    }
}

fn in_chain(o: Option<i32>) {
    let mc = ManuallyDrop::new(String::from("mc"));
    if let Some(_) = o && take(mc) {
        println!("taken");
    }
}

fn in_loop(c: bool) {
    let ml = ManuallyDrop::new(String::from("ml"));
    loop {
        if c {
            take(ml);
            break;
        }
        break;
    }
}

fn assigned_in_if(c: bool) {
    let mi;
    if c {
        mi = ManuallyDrop::new(String::from("mi"));
        println!("{}", *mi);
    }
}

fn assigned_in_loop(n: i32) {
    let mut mo;
    for _ in 0..n {
        mo = ManuallyDrop::new(String::from("mo"));
        println!("{}", *mo);
    }
}

fn field_moved() {
    let s = S { a: String::from("a"), n: NoDrop(1), m: ManuallyDrop::new(String::from("m")) };
    drop(s.a);
}

fn field_maybe_moved(c: bool) {
    let t = S { a: String::from("a"), n: NoDrop(1), m: ManuallyDrop::new(String::from("m")) };
    if c {
        drop(t.a);
    }
}

fn md_field_maybe_moved(c: bool) {
    let u = S { a: String::from("a"), n: NoDrop(1), m: ManuallyDrop::new(String::from("m")) };
    if c {
        take(u.m);
    }
}

fn plain_field_maybe_moved(c: bool) {
    let v = S { a: String::from("a"), n: NoDrop(1), m: ManuallyDrop::new(String::from("m")) };
    if c {
        take_n(v.n);
    }
}

fn main() {
    in_if(true);
    in_arm(Some(1));
    in_chain(Some(1));
    in_loop(true);
    assigned_in_if(true);
    assigned_in_loop(1);
    field_moved();
    field_maybe_moved(true);
    md_field_maybe_moved(true);
    plain_field_maybe_moved(true);
}
