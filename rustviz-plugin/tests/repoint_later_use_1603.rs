// Re-points in one arm whose old loan lives on past the other arm's read
// (Codex review of #1603): a later read of r or of x (whose loan the copy
// r carries) after the `if`, the struct used again in the re-pointing arm,
// a two-lender field, an arm that returns, a nested conditional that gives
// r a new value on every path.

struct Holder<'a> {
    r: &'a String,
}

fn longest<'a>(a: &'a String, b: &'a String) -> &'a String {
    if a.len() > b.len() { a } else { b }
}

// r is read after the `if`: on the `else` path its borrow doesn't end at
// the `else` arm's read.
fn copy_read_after(s: String, t: String, c: bool) {
    let x = &s;
    let mut r = x;
    if c {
        r = &t;
    } else {
        println!("{}", r);
    }
    println!("{}", r);
}

// x is read after the `if`: s stays borrowed by x when r's borrow ends.
fn original_read_after(s: String, t: String, c: bool) {
    let x = &s;
    let mut r = x;
    if c {
        r = &t;
    } else {
        println!("{}", r);
    }
    println!("{}", x);
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

// Both of longest's lenders are given back at the other arm's read.
fn two_lenders_read_other(s: String, t: String, u: String, c: bool) {
    let mut h = Holder { r: longest(&s, &t) };
    if c {
        h.r = &u;
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

// (Codex r2 on #1603.) Another reference to t read after the `if`: t
// stays borrowed by it where the field's loan ends.
fn other_ref_read_after(s: String, t: String, u: String, c: bool) {
    let other = &t;
    let mut h = Holder { r: longest(&s, &t) };
    if c {
        h.r = &u;
    } else {
        println!("{}", h.r);
    }
    println!("{}", other);
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

// x read only on the path that re-points r: the other path's print is
// the shared loan's last use, and s is free for its push.
fn original_read_on_other_path(mut s: String, t: String, c: bool) {
    let x = &s;
    let mut r = x;
    if c {
        println!("{}", r);
        s.push('a');
    } else {
        r = &t;
        println!("{}", x);
    }
}

// r's old value unread in the arm that re-points it: its loan is over as
// the arm starts, before the arm's push.
fn lender_used_before_repoint(mut s: String, t: String, c: bool) {
    let x = &s;
    let mut r = x;
    if c {
        s.push('a');
        r = &t;
    } else {
        println!("{}", r);
    }
}

fn main() {}
