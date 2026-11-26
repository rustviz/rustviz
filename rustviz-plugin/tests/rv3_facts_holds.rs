// RustViz 3 facts: what each variable's value keeps borrowed after a row
// (`holds`), as the borrow checker counts it (`facts_pin_holds`).
struct H<'a> {
    r: &'a String,
}

// One-line arms: each arm holds its own loan; the row (after the join), both.
fn arms(c: bool) {
    let x = String::from("x");
    let y = String::from("y");
    let r;
    if c { r = &x; } else { r = &y; }
    println!("{}", r);
}

// The arm re-points r before reading it: on the arm's path, r's loan of x
// is dead after `if c {` (the read below the `if` is on the other path).
fn skipped(c: bool) {
    let x = String::from("x");
    let y = String::from("y");
    let mut r = &x;
    if c {
        r = &y;
        println!("{}", r);
    }
    println!("{}", r);
}

// A moved-out reference holds nothing; the one it moved to holds the loan.
fn moved() {
    let mut s = String::from("s");
    let m = &mut s;
    let n = m;
    n.push('a');
}

// The same for a parameter whose referent's origin outlives the body: once
// h is moved out, it holds nothing.
fn moved_param<'a>(h: &mut H<'a>, t: &'a String) {
    h.r = t;
    let moved = h;
    println!("{}", moved.r);
}

// rustc's view: `h.r = &b` leaves a's loan in h, and the copy made before
// it gets b's too (a write to a or b here is E0502 while h and old live).
fn overwrite() {
    let a = String::from("a");
    let b = String::from("b");
    let mut h = H { r: &a };
    let old = h.r;
    h.r = &b;
    println!("{} {}", h.r, old);
}

pub fn main() {
    arms(true);
    skipped(true);
    moved();
    let (a, b) = (String::new(), String::new());
    moved_param(&mut H { r: &a }, &b);
    overwrite();
}
