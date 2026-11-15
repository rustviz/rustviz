// `v = v;` (rustc warns: useless assignment of a variable to itself, #1021)
// moves v's String out and straight back in: nothing is dropped, and v owns
// the same String afterwards (checked with a Drop-printing type). Nothing is
// drawn on v for it, as for a branch that gives v its own value back in
// `v = if c { v } else { g };` (#1010).

// In a branch: after the `if`, v owns a String on both paths.
fn keep2(c: bool, g: String) -> String {
    let mut v = String::from("old");
    if c {
        v = v;
    } else {
        v = g;
    }
    v
}

// Not in a branch: w still owns its String and drops it at its scope end.
fn top() {
    let mut w = String::from("old");
    w = w;
    println!("{w}");
}

// A one-armed `if`: t owns its String after it on every path.
fn one_arm(c: bool) {
    let mut t = String::from("old");
    if c {
        t = t;
    }
    println!("{t}");
}

// A field assigned to itself: p.name keeps its String.
struct P {
    name: String,
}

fn field() {
    let mut p = P { name: String::from("a") };
    p.name = p.name;
    println!("{}", p.name);
}

// A Copy value assigned to itself is copied, as any Copy assignment is.
fn copy(c: bool) -> i32 {
    let mut n = 5;
    if c {
        n = n;
    }
    n
}
