// #1034: a variable that may hold no value where a conditional starts
// (a branch above may have moved it), given a new value in only some of
// its branches. After the conditional it may hold the new value, the
// old one, or nothing, and only a drop flag knows. Checked with rustc
// and a `Drop`-printing type (each branch outcome, loops run 0, 1 and
// 2 times).

// c = false, x = false: "a" is dropped at the scope end, not at an
// assignment. c = true, x = false: s holds nothing there, and nothing
// is dropped.
fn one_armed(c: bool, x: bool) {
    let mut s = String::from("a");
    if c {
        drop(s);
    }
    if x {
        s = String::from("c");
    }
}

// The same with an `else` that leaves s alone.
fn with_else(c: bool, x: bool) {
    let mut t = String::from("a");
    if c {
        drop(t);
    }
    if x {
        t = String::from("c");
    } else {
        println!("no");
    }
}

// Assigned again after the `if` on every path: what u holds after the
// `if` ("a", "c" or nothing) is dropped there, if anything.
fn assigned_after(c: bool, x: bool) {
    let mut u = String::from("a");
    if c {
        drop(u);
    }
    if x {
        u = String::from("c");
    }
    u = String::from("d");
    println!("{}", u);
}

// In a loop: the next time round's assignment drops what v holds after
// the `if`, if anything (c = false, o = 2, x = true: the first "c").
fn in_loop(c: bool, o: usize, x: bool) {
    let mut v = String::from("a");
    if c {
        drop(v);
    }
    for _ in 0..o {
        if x {
            v = String::from("c");
        }
    }
}

// Moved on every path before the `if`: w holds nothing unless x.
fn moved_before(x: bool) {
    let mut w = String::from("a");
    drop(w);
    if x {
        w = String::from("c");
    }
}

// The issue's loop shape: an inner loop that may not run (rustc doesn't
// know it runs `i` times), then the `if`. After the `if`, q may hold
// nothing (c = true, i = 0, x = false), "b", or "c"; the next time
// round's assignment drops it, if anything, or else q's scope end.
fn after_inner_loop(c: bool, o: usize, i: usize, x: bool) {
    let mut q = String::from("a");
    if c {
        drop(q);
    }
    for _ in 0..o {
        let mut n = 0;
        while n < i {
            q = String::from("b");
            n += 1;
        }
        if x {
            q = String::from("c");
        }
    }
}
