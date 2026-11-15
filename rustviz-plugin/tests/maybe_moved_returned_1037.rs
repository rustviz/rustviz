// #1037: a variable that may hold no value (a branch above may have
// moved it), given a new value only in a branch of a later conditional
// that then returns or panics. The branches that get past that
// conditional leave the variable as it was, so at the scope end it
// still may hold its old value: a drop flag decides. Also for a `let
// mut s;` local an `if` or a loop above may have assigned. Checked with
// rustc and a `Drop`-printing type (each branch outcome, loops run 0,
// 1 and 2 times).

// c = false, x = false: "a" is dropped at the scope end. c = true:
// nothing is.
fn ret(c: bool, x: bool) -> String {
    let mut s = String::from("a");
    if c {
        drop(s);
    }
    if x {
        s = String::from("c");
        return s;
    }
    String::new()
}

// With an `else` that leaves t alone: c = false, x = false drops "a".
fn with_else(c: bool, x: bool) -> String {
    let mut t = String::from("a");
    if c {
        drop(t);
    }
    if x {
        t = String::from("c");
        return t;
    } else {
        println!("no");
    }
    String::new()
}

// The branch panics instead: c = false, x = false drops "a".
fn panics(c: bool, x: bool) {
    let mut u = String::from("a");
    if c {
        drop(u);
    }
    if x {
        u = String::from("c");
        panic!("{}", u);
    }
}

// In a `match`: c = false, k = 1 drops "a".
fn in_match(c: bool, k: u8) -> String {
    let mut v = String::from("a");
    if c {
        drop(v);
    }
    match k {
        0 => {
            v = String::from("c");
            return v;
        }
        _ => println!("no"),
    }
    String::new()
}

// A `let mut w;` an `if` above may have assigned: a = true, x = false
// drops "a" at the scope end; a = false, x = false drops nothing.
fn deferred(a: bool, x: bool) -> String {
    let mut w;
    if a {
        w = String::from("a");
    }
    if x {
        w = String::from("c");
        return w;
    }
    String::new()
}

// A `let mut q;` a loop above may have assigned: n = 1, x = false
// drops "w" at the scope end; n = 0, x = false drops nothing.
fn after_loop(n: usize, x: bool) -> String {
    let mut q;
    for _ in 0..n {
        q = String::from("w");
    }
    if x {
        q = String::from("c");
        return q;
    }
    String::new()
}

fn main() {
    println!("{}", ret(false, false));
    println!("{}", with_else(false, false));
    panics(false, false);
    println!("{}", in_match(false, 1));
    println!("{}", deferred(true, false));
    println!("{}", after_loop(1, false));
}
