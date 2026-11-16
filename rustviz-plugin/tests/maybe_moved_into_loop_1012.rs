// #1012: a variable moved in a branch before a loop (so it may or may
// not hold a value as the loop starts) and assigned in the loop's body.
// The first time round, the assignment drops the old value only if the
// branch didn't move it; a later time round, what the time round before
// left. After a `while` / `for` (or a `break` before the assignment),
// the variable holds a value only if the loop assigned it or the branch
// didn't move it: a drop flag decides.

// The bare loop of #1012: the second time round, `drop(s)` has moved s.
fn entry_maybe(c: bool, k: usize) {
    let mut s = String::from("a");
    if c {
        drop(s);
    }
    let mut n = 0;
    loop {
        s = String::from("b");
        if n >= k {
            break;
        }
        drop(s);
        n += 1;
    }
    println!("{}", s);
}

// A `while`: the second time round t holds the first time round's value.
fn entry_while(c: bool, k: usize) {
    let mut t = String::from("a");
    if c {
        drop(t);
    }
    let mut n = 0;
    while n < k {
        t = String::from("b");
        n += 1;
    }
}

fn entry_for(c: bool, k: usize) {
    let mut u = String::from("a");
    if c {
        drop(u);
    }
    for _ in 0..k {
        u = String::from("b");
    }
}

// A bare loop whose `break` comes before the assignment.
fn break_first(c: bool, k: usize) {
    let mut v = String::from("a");
    if c {
        drop(v);
    }
    let mut n = 0;
    loop {
        if n >= k {
            break;
        }
        v = String::from("b");
        n += 1;
    }
}

// Every time round moves what it assigned.
fn while_moves(c: bool, k: usize) {
    let mut w = String::from("a");
    if c {
        drop(w);
    }
    let mut n = 0;
    while n < k {
        w = String::from("b");
        drop(w);
        n += 1;
    }
}

// Assigned again after the loop.
fn assign_after(c: bool, k: usize) {
    let mut x = String::from("a");
    if c {
        drop(x);
    }
    let mut n = 0;
    while n < k {
        x = String::from("b");
        n += 1;
    }
    x = String::from("c");
    println!("{}", x);
}

// Assigned only in some time rounds.
fn in_branch(c: bool, k: usize) {
    let mut y = String::from("a");
    if c {
        drop(y);
    }
    let mut n = 0;
    while n < k {
        if n % 2 == 0 {
            y = String::from("b");
        }
        n += 1;
    }
}

struct P {
    name: String,
}

fn whole_struct(c: bool, k: usize) {
    let mut p = P { name: String::from("a") };
    if c {
        drop(p);
    }
    let mut n = 0;
    while n < k {
        p = P { name: String::from("b") };
        n += 1;
    }
}

fn main() {
    entry_maybe(false, 1);
    entry_while(true, 0);
    entry_for(true, 2);
    break_first(false, 0);
    while_moves(false, 0);
    assign_after(true, 0);
    in_branch(true, 0);
    whole_struct(true, 0);
}
