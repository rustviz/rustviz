// A `loop` that nothing breaks out of, left only at exits drawn in a
// branch (`return`, panic) or at a `?`, or never (#1057). The code never
// gets past the loop, so nothing is dropped after it (#1043), but on the
// paths that keep looping every variable declared before it is still in
// scope (an owner still owns its resource): its column runs on to the
// loop's closing `}` and ends there with a plain dot, not a drop.

// g is dropped only at the `return`; with c false the loop runs forever
// and g stays alive.
fn onlybr(g: String, c: bool) -> usize {
    loop {
        if c {
            return g.len();
        }
    }
}

// Only a `?`: w is dropped there if it returns early.
fn only_try(w: String, x: Option<u8>) -> Option<u8> {
    loop {
        x?;
    }
}

// Nothing leaves this loop: g and n never go out of scope.
fn spin(g: String) -> usize {
    let n = g.len();
    loop {
    }
}

// The `return` moves g to the caller: it isn't dropped there.
fn give(g: String, c: bool) -> String {
    loop {
        if c {
            return g;
        }
    }
}

// `process::exit` drops nothing, and `n += 1` may panic (overflow), which
// isn't drawn: just "still alive".
fn quit(g: String, c: bool) -> usize {
    let mut n = g.len();
    loop {
        n += 1;
        if c {
            std::process::exit(1);
        }
    }
}

// Moved before the loop: h is the one still alive; g has nothing left.
fn moved_before(g: String, c: bool) -> usize {
    let h = g;
    loop {
        if c {
            return h.len();
        }
    }
}

// Nested: s (declared in the outer loop's body) is still alive while the
// inner loop runs; its line, and c's, end at the inner loop's `}`.
fn nested(c: u8) -> usize {
    loop {
        let s = String::from("s");
        if c == 1 {
            return s.len();
        }
        loop {
            if c == 2 {
                return 1;
            }
        }
    }
}

// In a `for` body: u ends at the inner loop's `}`; the `for` body's own
// `}` is never reached. (The `for` loop may not run: f and k reach the
// end.)
fn in_for(v: Vec<u8>, f: String, k: bool) -> usize {
    for x in v {
        let u = String::from("u");
        loop {
            if k {
                return u.len() + x as usize;
            }
        }
    }
    f.len()
}
