// #1460: a conditional that may move a value, followed by a later
// conditional that may assign it. No line reaches the first one's join
// (the value is maybe-moved after it), so its text went on the column's
// next dot, inside the later conditional: on the first dot of each of
// its arms and on its join, as if about that conditional. It now stays
// on its own row. Its text said the old value is dropped "when o goes
// out of scope" when the later conditional is nested in an outer arm,
// though on the path that assigns o it is dropped there, and "(or, if
// it isn't, when it goes out of scope)" before an `if` / `else` that
// assigns on both paths.

fn id<T>(t: T) -> T {
    t
}

fn main() {
    // The issue's repro: a let-chain `if` in an `else if` arm.
    let s = String::from("rust");
    let mut o = Some(id(s));
    if o.is_some() {
    } else if o.is_some() {
        if let Some(x3) = o && x3.len() > 1 {
        }
        if true {
            o = Some(String::from("rust"));
        }
    }
}

// At the top level.
fn top(c: bool) {
    let mut a = Some(String::from("a"));
    if let Some(x) = a && x.len() > 1 {
    }
    if c {
        a = Some(String::from("b"));
    }
}

fn take(s: Option<String>) -> bool {
    s.is_some()
}

// A `match` guard that moves g and then fails, then a `match` that
// assigns g in one arm.
fn guard(c: bool) {
    let mut g = Some(String::from("g"));
    match c {
        true if take(g) => {}
        _ => {}
    }
    match c {
        true => g = Some(String::from("h")),
        false => {}
    }
}

// Then an `if` / `else` that assigns b on both paths.
fn both(c: bool) {
    let mut b = Some(String::from("b"));
    if let Some(x) = b && x.len() > 1 {
    }
    if c {
        b = Some(String::from("c"));
    } else {
        b = None;
    }
    println!("{:?}", b);
}

// A plain `if` that moves p, in an `else if` arm, then an `if` in that
// arm that assigns it.
fn plain(c: bool) {
    let mut p = Some(String::from("p"));
    if c {
    } else if p.is_some() {
        if c {
            drop(p);
        }
        if c {
            p = Some(String::from("q"));
        }
    }
}

// A let-chain `if` with an `else`, in an `if` arm.
fn with_else(c: bool) {
    let mut e = Some(String::from("e"));
    if c {
        if let Some(x) = e && x.len() > 1 {
            println!("{}", x);
        } else {
            println!("no");
        }
        if c {
            e = Some(String::from("f"));
        }
    }
}
