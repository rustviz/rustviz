// Known bug, from the closed 2.x fix PRs (fix/r2-joins-1582-1584-1587-1590); kept as a RustViz 3 regression case.
// Issue(s): #1587 (see known_bugs/README.md for what main draws wrong).
// #1587: a loop that may end before giving s a value, then an `if` that
// gives it one in some branches only. Before line 13, s holds a value only
// if the loop ran or c was true: the assignment there drops the old value
// through a drop flag, as do the `match` arm's and the plain assignment's
// below. Main said "its previous resource is dropped", unconditionally.
// (The loops run `n` times, which may be 0: then s has no value there.)
// After an `if` alone (no loop), the arm's assignment is in the same
// position: the old value is there only if s still holds it, not "only if
// no branch above moved it" (none did).
fn run(c: bool, d: bool, n: u8) {
    let mut s = String::from("a");
    drop(s);
    for _i in 0..n {
        s = String::from("b");
    }
    if c {
        s = String::from("b");
    }
    if d {
        s = String::from("c");
    } else {
    }
}

fn in_match_arm(c: bool, k: u8, n: u8) {
    let mut m = String::from("a");
    drop(m);
    for _i in 0..n {
        m = String::from("b");
    }
    if c {
        m = String::from("b");
    }
    match k {
        0 => m = String::from("c"),
        _ => {}
    }
}

fn after_the_if(c: bool, mut n: u8) {
    let mut u = String::from("a");
    drop(u);
    while n > 0 {
        u = String::from("b");
        n -= 1;
    }
    if c {
        u = String::from("b");
    }
    u = String::from("c");
    println!("{}", u);
}

fn no_loop(c: bool, d: bool) {
    let mut w = String::from("a");
    drop(w);
    if c {
        w = String::from("b");
    }
    if d {
        w = String::from("c");
    } else {
    }
}
