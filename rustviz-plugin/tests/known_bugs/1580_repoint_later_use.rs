// Known bug, from the closed 2.x fix PRs (fix/r3-repoint); kept as a RustViz 3 regression case.
// Issue(s): #1580 (see known_bugs/README.md for what main draws wrong).
// Re-points in one arm whose old loan lives on (Codex review of #1603):
// the struct used again in the re-pointing arm, also after a nested `if`,
// an arm that returns, a nested conditional that gives r a new value on
// every path, a re-borrow of the same lender.

struct Holder<'a> {
    r: &'a String,
}

fn longest<'a>(a: &'a String, b: &'a String) -> &'a String {
    if a.len() > b.len() { a } else { b }
}

// h is used again after the write in the first arm: there the old loan
// lives up to that use.
fn field_read_in_both(s: String, t: String, c: bool) {
    let mut h = Holder { r: &s };
    if c {
        h.r = &t;
        println!("{}", h.r);
    } else {
        println!("{}", h.r);
    }
}

// The arm that returns runs up to its `return`: s is free in it.
fn other_arm_returns(mut s: String, t: String, c: bool) {
    let mut h = Holder { r: &s };
    if c {
        h.r = &t;
        println!("{}", h.r);
    } else {
        s.push('z');
        return;
    }
}

// Every path through the nested `if` gives r a new value before the
// read: the old loan ends at the condition.
fn new_value_on_every_path(s: String, t: String, u: String, a: bool, b: bool) {
    let mut r = &s;
    if a {
        if r.is_empty() {
            if b {
                r = &t;
            } else {
                r = &u;
            }
            println!("{}", r);
        } else {
        }
    } else {
    }
}

// A field re-pointed at the same lender: `&s` is taken before the write
// ends the old loan, so s is borrowed by h.r again before it is given
// back, never free in between.
fn same_lender_again(s: String) {
    let mut h = Holder { r: &s };
    let n = 1;
    h.r = &s;
}

// h.r re-pointed in a nested arm, read after the nested `if`: on the
// nested `else` path it still points to s there.
fn read_after_nested_join(s: String, t: String, a: bool, b: bool) {
    let mut h = Holder { r: &s };
    if a {
        if b {
            h.r = &t;
        } else {
        }
        println!("{}", h.r);
    } else {
        println!("{}", h.r);
    }
}

fn main() {}
