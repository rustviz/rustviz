// A struct's reference field re-pointed in one arm (`h.r = &t;`). A write
// to a field uses its struct, so the borrow checker keeps the loans h
// holds live up to the write: the old loan ends there, for each of its
// lenders (both of `longest(&s, &t)`'s), and as the other arms start
// (#1588). It used to end before the `if`, t stayed "shared" to the fn's
// end on the other path, and the old loan of t ended twice.

struct Holder<'a> {
    r: &'a String,
}

fn longest<'a>(a: &'a String, b: &'a String) -> &'a String {
    if a.len() > b.len() { a } else { b }
}

fn two_lenders(s: String, t: String, c: bool) {
    let mut h = Holder { r: longest(&s, &t) };
    if c {
        h.r = &t;
    } else {
    }
}

fn two_lenders_else_if(s: String, t: String, c: bool, d: bool) {
    let mut h = Holder { r: longest(&s, &t) };
    if c {
        h.r = &t;
    } else if d {
    } else {
    }
}

fn one_lender(s: String, t: String, c: bool) {
    let mut h = Holder { r: &s };
    if c {
        h.r = &t;
    } else {
    }
}

// h used after the write in the same arm: the old loan lives to that use
// (#531); on the `else` path it ends as the arm starts.
fn read_after_the_write(s: String, t: String, c: bool) {
    let mut h = Holder { r: &s };
    if c {
        h.r = &t;
        println!("{}", h.r);
    } else {
    }
}

// The field copied from another reference in an `else if` chain's last
// arm: its old loan of s and the new one of t end on that row, each
// naming its own lender.
fn copied_in_final_else(s: String, t: String, flag: bool) {
    let mut h = Holder { r: &s };
    let r = &t;
    if flag {
    } else if t.len() > 1 {
    } else {
        h.r = r;
    }
}

fn main() {}
