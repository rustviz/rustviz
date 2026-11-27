// #1871's review: one-line conditionals whose arms got rows of their own.

// c is borrowed after the conditional (`&c` is the tuple's second
// element): above its dot c is still free.
fn borrow_after_arms(flag: bool) {
    let mut a = String::from("a");
    let b = String::from("b");
    let c = String::from("c");
    let r: &String = &mut a;
    let mut w: Vec<(&String, &String)> = Vec::new();
    w.push((if flag { r } else { &b }, &c));
    println!("{:?}", w);
}

// Two conditionals on one line: the first's arms differ (s or t lent to
// x), the second's agree (s.len() on one arm, nothing on the other).
fn siblings(c: bool, d: bool) {
    let s = String::from("s");
    let t = String::from("t");
    let (x, y) = (if c { &s } else { &t }, if d { s.len() } else { 0 });
    println!("{} {}", x, y);
}

// v is borrowed by maybe only on the `if choose` arm; the `None` arm's
// value holds no reference.
fn maybe_none(v: Vec<String>, choose: bool) {
    let maybe = if choose { v.first() } else { None };
    println!("{:?}", maybe);
}

// The `?` gives the first arm a second way out (an early return): its
// state on the way to the join is s owned and lent to r, the same as the
// other arm's, so the arms share a row.
fn early_exit_arm(o: Option<u8>, c: bool) -> Option<u8> {
    let s = String::from("a");
    let r = &s;
    let n = if c { o?; 1 } else { 2 };
    println!("{} {}", r, n);
    Some(n)
}

// The dead `else` of an irrefutable `if let`, with its `{` on the line
// after the keyword: never runs (value has no value only on the path the
// borrow checker imagines through it).
#[allow(irrefutable_let_patterns)]
fn dead_else_next_line(a: String, g: String) {
    let value;
    if let x = a {
        value = x;
    }
    else
    {
        println!("{}", g);
        value = String::new();
    }
    println!("{}", value);
}

// q is given r's reference on one arm only: the arms leave q holding
// different borrows (and s held by q too on one), so each gets a row.
fn copy_holder(c: bool) {
    let s = String::from("s");
    let t = String::from("t");
    let r = &s;
    let mut q = &t;
    if c { q = r; } else { println!("{}", q); }
    println!("{} {}", q, r);
}

fn main() {
    borrow_after_arms(true);
    siblings(true, false);
    maybe_none(Vec::new(), true);
    early_exit_arm(Some(1), true);
    dead_else_next_line(String::new(), String::new());
    copy_holder(true);
}
