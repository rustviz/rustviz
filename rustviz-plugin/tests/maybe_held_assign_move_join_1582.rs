// #1582: s may hold a value only if c was true. The second conditional's
// first arm gives it a new value and moves that away; the others leave it
// as it was. So after it s holds "b" only if c and not d, and a drop flag
// drops it at the scope end. Main drew no join dot there (the `else` leg
// stopped in mid-air), said the arm's assignment drops the old value
// "only if no branch above moved it" (none did), and so did the scope end.
// Also with a `?` in an arm instead (no assignment), and for a `let x;`
// local, where main said "each of them moved it away, so x holds nothing"
// (and a `?` after it drops x only if c and not d). In `may_move` the arm
// only may move y: d && !e keeps the new String.
fn run(c: bool, d: bool) {
    let mut s = String::from("a");
    drop(s);
    if c {
        s = String::from("b");
    }
    if d {
        s = String::from("c");
        drop(s);
    } else {
    }
}

fn in_match_arm(c: bool, k: u8) {
    let mut m = String::from("a");
    drop(m);
    if c {
        m = String::from("b");
    }
    match k {
        0 => {
            m = String::from("c");
            drop(m);
        }
        _ => {}
    }
}

fn try_in_arm(c: bool, k: Option<u8>, v: Vec<u8>) -> Option<usize> {
    let mut t = String::from("x");
    drop(t);
    if c {
        t = String::from("y");
    }
    match k {
        Some(_) => {
            let _r = v.last()?;
        }
        None => {}
    }
    Some(0)
}

fn try_in_else(c: bool, d: bool, o: Option<u8>) -> Option<u8> {
    let mut e = String::from("a");
    drop(e);
    if c {
        e = String::from("b");
    }
    if d {
    } else {
        let _n = o?;
    }
    Some(0)
}

fn deferred_local(c: bool, d: bool, o: Option<u8>) -> Option<u8> {
    let mut x;
    if c {
        x = String::from("b");
    }
    if d {
        x = String::from("c");
        drop(x);
    } else {
    }
    o?;
    Some(0)
}

fn may_move(c: bool, d: bool, e: bool) {
    let mut y = String::from("a");
    drop(y);
    if c {
        y = String::from("b");
    }
    if d {
        y = String::new();
        if e {
            drop(y);
        }
    } else {
    }
}

fn no_move(c: bool, d: bool, e: bool) {
    let mut z;
    if c {
        z = String::new();
    }
    if d {
        if e {
            z = String::new();
        }
    }
}

// #1600 review: a three-arm match where one arm leaves x alone.
fn three_arms(c: bool, k: u8) {
    let mut x;
    if c {
        x = String::new();
    }
    match k {
        0 => x = String::new(),
        1 => {
            x = String::new();
            drop(x);
        }
        _ => {}
    }
}

// A `?` between the join and the next assignment.
fn try_before_assign(c: bool, d: bool, o: Option<u8>) -> Option<u8> {
    let mut q;
    if c {
        q = String::new();
    }
    if d {
        q = String::new();
    }
    o?;
    q = String::new();
    drop(q);
    Some(0)
}
