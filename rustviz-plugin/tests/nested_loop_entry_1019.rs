// #1019: a variable a branch may have moved (so it may or may not hold
// a value), assigned in a loop inside another loop that it was declared
// before. A run of the inner loop may follow an earlier one that gave
// it a value, so "the first time round the loop, only if no branch
// above moved it" isn't true of the inner loop's first time round: on
// the outer loop's second time round it drops what the first left.
// And a loop that moves it on some time rounds only: after it, whether
// it holds a value isn't settled by whether "no branch above" moved it.
// (Known wrong, not pinned: the join after each `if n == 0 { drop(..); }`
// says the value is dropped when the variable goes out of scope; when
// the loop comes back round, the assignment at the top drops it, #1026.)

// The issue's program: the loops never move s.
fn nested(c: bool, k: usize) {
    let mut s = String::from("a");
    if c {
        drop(s);
    }
    for _ in 0..k {
        let mut n = 0;
        while n < k {
            s = String::from("b");
            n += 1;
        }
    }
}

// The inner loop moves s on its first time round: only the drop flag
// knows.
fn nested_moves(c: bool, k: usize) {
    let mut s = String::from("a");
    if c {
        drop(s);
    }
    for _ in 0..k {
        let mut n = 0;
        while n < k {
            s = String::from("b");
            if n == 0 {
                drop(s);
            }
            n += 1;
        }
    }
}

// A bare outer loop left by a `break`.
fn outer_bare(c: bool, k: usize) {
    let mut v = String::from("a");
    if c {
        drop(v);
    }
    let mut m = 0;
    loop {
        if m >= k {
            break;
        }
        for _ in 0..k {
            v = String::from("b");
        }
        m += 1;
    }
}

struct P {
    name: String,
}

// A struct and its field column.
fn nested_struct(c: bool, k: usize) {
    let mut p = P { name: String::from("a") };
    if c {
        drop(p);
    }
    for _ in 0..k {
        for _ in 0..k {
            p = P { name: String::from("b") };
        }
    }
}

// The issue's second program: one loop, which moves t on its first time
// round only.
fn some_rounds(c: bool, k: usize) {
    let mut t = String::from("a");
    if c {
        drop(t);
    }
    let mut n = 0;
    while n < k {
        t = String::from("b");
        if n == 0 {
            drop(t);
        }
        n += 1;
    }
}

// The same, assigned again after the loop.
fn some_rounds_then(c: bool, k: usize) {
    let mut w = String::from("a");
    if c {
        drop(w);
    }
    let mut n = 0;
    while n < k {
        w = String::from("b");
        if n == 0 {
            drop(w);
        }
        n += 1;
    }
    w = String::from("c");
    println!("{}", w);
}

// No branch above (#992): the loop's own moves only.
fn plain_some(k: usize) {
    let mut u = String::from("a");
    let mut n = 0;
    while n < k {
        u = String::from("b");
        if n == 0 {
            drop(u);
        }
        n += 1;
    }
}
